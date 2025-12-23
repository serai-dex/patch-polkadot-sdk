// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! A consensus proposer for "basic" chains which use the primitive inherent-data.

// FIXME #1021 move this into sp-consensus

use codec::Encode;
use futures::{
	channel::oneshot,
	future,
	future::{Future, FutureExt},
};
use log::{debug, error, info, log_enabled, trace, warn, Level};
use prometheus_endpoint::Registry as PrometheusRegistry;
use sc_block_builder::{BlockBuilderApi, BlockBuilderBuilder};
use sc_proposer_metrics::{EndProposingReason, MetricsLink as PrometheusMetrics};
use sc_telemetry::{telemetry, TelemetryHandle, CONSENSUS_INFO};
use sc_transaction_pool_api::{InPoolTransaction, TransactionPool, TxInvalidityReportMap};
use sp_api::{ApiExt, CallApiAt, ProofRecorder, ProvideRuntimeApi};
use sp_blockchain::{ApplyExtrinsicFailed::Validity, Error::ApplyExtrinsicFailed, HeaderBackend};
use sp_consensus::{DisableProofRecording, EnableProofRecording, ProofRecording, Proposal};
use sp_core::traits::SpawnNamed;
use sp_inherents::InherentData;
use sp_runtime::{
	traits::{BlakeTwo256, Block as BlockT, Hash as HashT, Header as HeaderT},
	Digest, ExtrinsicInclusionMode, Percent, SaturatedConversion,
};
use sp_trie::recorder::IgnoredNodes;
use std::{marker::PhantomData, pin::Pin, sync::Arc, time};

/// Default block size limit in bytes used by [`Proposer`].
///
/// Can be overwritten by [`ProposerFactory::set_default_block_size_limit`].
///
/// Be aware that there is also an upper packet size on what the networking code
/// will accept. If the block doesn't fit in such a package, it can not be
/// transferred to other nodes.
pub const DEFAULT_BLOCK_SIZE_LIMIT: usize = 4 * 1024 * 1024 + 512;

const DEFAULT_SOFT_DEADLINE_PERCENT: Percent = Percent::from_percent(50);

const LOG_TARGET: &'static str = "basic-authorship";

/// [`Proposer`] factory.
pub struct ProposerFactory<A, C, PR> {
	spawn_handle: Box<dyn SpawnNamed>,
	/// The client instance.
	client: Arc<C>,
	/// The transaction pool.
	transaction_pool: Arc<A>,
	/// Prometheus Link,
	metrics: PrometheusMetrics,
	/// The default block size limit.
	///
	/// If no `block_size_limit` is passed to [`sp_consensus::Proposer::propose`], this block size
	/// limit will be used.
	default_block_size_limit: usize,
	/// Soft deadline percentage of hard deadline.
	///
	/// The value is used to compute soft deadline during block production.
	/// The soft deadline indicates where we should stop attempting to add transactions
	/// to the block, which exhaust resources. After soft deadline is reached,
	/// we switch to a fixed-amount mode, in which after we see `MAX_SKIPPED_TRANSACTIONS`
	/// transactions which exhaust resources, we will conclude that the block is full.
	soft_deadline_percent: Percent,
	telemetry: Option<TelemetryHandle>,
	/// When estimating the block size, should the proof be included?
	include_proof_in_block_size_estimation: bool,
	/// phantom member to pin the `ProofRecording` type.
	_phantom: PhantomData<PR>,
}

impl<A, C, PR> Clone for ProposerFactory<A, C, PR> {
	fn clone(&self) -> Self {
		Self {
			spawn_handle: self.spawn_handle.clone(),
			client: self.client.clone(),
			transaction_pool: self.transaction_pool.clone(),
			metrics: self.metrics.clone(),
			default_block_size_limit: self.default_block_size_limit,
			soft_deadline_percent: self.soft_deadline_percent,
			telemetry: self.telemetry.clone(),
			include_proof_in_block_size_estimation: self.include_proof_in_block_size_estimation,
			_phantom: self._phantom,
		}
	}
}

impl<A, C> ProposerFactory<A, C, DisableProofRecording> {
	/// Create a new proposer factory.
	///
	/// Proof recording will be disabled when using proposers built by this instance to build
	/// blocks.
	pub fn new(
		spawn_handle: impl SpawnNamed + 'static,
		client: Arc<C>,
		transaction_pool: Arc<A>,
		prometheus: Option<&PrometheusRegistry>,
		telemetry: Option<TelemetryHandle>,
	) -> Self {
		ProposerFactory {
			spawn_handle: Box::new(spawn_handle),
			transaction_pool,
			metrics: PrometheusMetrics::new(prometheus),
			default_block_size_limit: DEFAULT_BLOCK_SIZE_LIMIT,
			soft_deadline_percent: DEFAULT_SOFT_DEADLINE_PERCENT,
			telemetry,
			client,
			include_proof_in_block_size_estimation: false,
			_phantom: PhantomData,
		}
	}
}

impl<A, C> ProposerFactory<A, C, EnableProofRecording> {
	/// Create a new proposer factory with proof recording enabled.
	///
	/// Each proposer created by this instance will record a proof while building a block.
	///
	/// This will also include the proof into the estimation of the block size. This can be disabled
	/// by calling [`ProposerFactory::disable_proof_in_block_size_estimation`].
	pub fn with_proof_recording(
		spawn_handle: impl SpawnNamed + 'static,
		client: Arc<C>,
		transaction_pool: Arc<A>,
		prometheus: Option<&PrometheusRegistry>,
		telemetry: Option<TelemetryHandle>,
	) -> Self {
		ProposerFactory {
			client,
			spawn_handle: Box::new(spawn_handle),
			transaction_pool,
			metrics: PrometheusMetrics::new(prometheus),
			default_block_size_limit: DEFAULT_BLOCK_SIZE_LIMIT,
			soft_deadline_percent: DEFAULT_SOFT_DEADLINE_PERCENT,
			telemetry,
			include_proof_in_block_size_estimation: true,
			_phantom: PhantomData,
		}
	}

	/// Disable the proof inclusion when estimating the block size.
	pub fn disable_proof_in_block_size_estimation(&mut self) {
		self.include_proof_in_block_size_estimation = false;
	}
}

impl<A, C, PR> ProposerFactory<A, C, PR> {
	/// Set the default block size limit in bytes.
	///
	/// The default value for the block size limit is:
	/// [`DEFAULT_BLOCK_SIZE_LIMIT`].
	///
	/// If there is no block size limit passed to [`sp_consensus::Proposer::propose`], this value
	/// will be used.
	pub fn set_default_block_size_limit(&mut self, limit: usize) {
		self.default_block_size_limit = limit;
	}

	/// Set soft deadline percentage.
	///
	/// The value is used to compute soft deadline during block production.
	/// The soft deadline indicates where we should stop attempting to add transactions
	/// to the block, which exhaust resources. After soft deadline is reached,
	/// we switch to a fixed-amount mode, in which after we see `MAX_SKIPPED_TRANSACTIONS`
	/// transactions which exhaust resources, we will conclude that the block is full.
	///
	/// Setting the value too low will significantly limit the amount of transactions
	/// we try in case they exhaust resources. Setting the value too high can
	/// potentially open a DoS vector, where many "exhaust resources" transactions
	/// are being tried with no success, hence block producer ends up creating an empty block.
	pub fn set_soft_deadline(&mut self, percent: Percent) {
		self.soft_deadline_percent = percent;
	}
}

impl<Block, C, A, PR> ProposerFactory<A, C, PR>
where
	A: TransactionPool<Block = Block> + 'static,
	Block: BlockT,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + Send + Sync + 'static,
	C::Api: ApiExt<Block> + BlockBuilderApi<Block>,
{
	fn init_with_now(
		&mut self,
		parent_header: &<Block as BlockT>::Header,
		now: Box<dyn Fn() -> time::Instant + Send + Sync>,
	) -> Proposer<Block, C, A, PR> {
		let parent_hash = parent_header.hash();

		info!(
			"Starting consensus session on top of parent {:?} (#{})",
			parent_hash,
			parent_header.number()
		);

		let proposer = Proposer::<_, _, _, PR> {
			spawn_handle: self.spawn_handle.clone(),
			client: self.client.clone(),
			parent_hash,
			parent_number: *parent_header.number(),
			transaction_pool: self.transaction_pool.clone(),
			now,
			metrics: self.metrics.clone(),
			default_block_size_limit: self.default_block_size_limit,
			soft_deadline_percent: self.soft_deadline_percent,
			telemetry: self.telemetry.clone(),
			_phantom: PhantomData,
			include_proof_in_block_size_estimation: self.include_proof_in_block_size_estimation,
		};

		proposer
	}
}

impl<A, Block, C, PR> sp_consensus::Environment<Block> for ProposerFactory<A, C, PR>
where
	A: TransactionPool<Block = Block> + 'static,
	Block: BlockT,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + CallApiAt<Block> + Send + Sync + 'static,
	C::Api: ApiExt<Block> + BlockBuilderApi<Block>,
	PR: ProofRecording,
{
	type CreateProposer = future::Ready<Result<Self::Proposer, Self::Error>>;
	type Proposer = Proposer<Block, C, A, PR>;
	type Error = sp_blockchain::Error;

	fn init(&mut self, parent_header: &<Block as BlockT>::Header) -> Self::CreateProposer {
		future::ready(Ok(self.init_with_now(parent_header, Box::new(time::Instant::now))))
	}
}

/// The proposer logic.
pub struct Proposer<Block: BlockT, C, A: TransactionPool, PR> {
	spawn_handle: Box<dyn SpawnNamed>,
	client: Arc<C>,
	parent_hash: Block::Hash,
	parent_number: <<Block as BlockT>::Header as HeaderT>::Number,
	transaction_pool: Arc<A>,
	now: Box<dyn Fn() -> time::Instant + Send + Sync>,
	metrics: PrometheusMetrics,
	default_block_size_limit: usize,
	include_proof_in_block_size_estimation: bool,
	soft_deadline_percent: Percent,
	telemetry: Option<TelemetryHandle>,
	_phantom: PhantomData<PR>,
}

impl<A, Block, C, PR> sp_consensus::Proposer<Block> for Proposer<Block, C, A, PR>
where
	A: TransactionPool<Block = Block> + 'static,
	Block: BlockT,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + CallApiAt<Block> + Send + Sync + 'static,
	C::Api: ApiExt<Block> + BlockBuilderApi<Block>,
	PR: ProofRecording,
{
	type Proposal =
		Pin<Box<dyn Future<Output = Result<Proposal<Block, PR::Proof>, Self::Error>> + Send>>;
	type Error = sp_blockchain::Error;
	type ProofRecording = PR;
	type Proof = PR::Proof;

	fn propose(
		self,
		inherent_data: InherentData,
		inherent_digests: Digest,
		max_duration: time::Duration,
		block_size_limit: Option<usize>,
	) -> Self::Proposal {
		self.propose_block(ProposeArgs {
			inherent_data,
			inherent_digests,
			max_duration,
			block_size_limit,
			ignored_nodes_by_proof_recording: None,
		})
		.boxed()
	}
}

/// Arguments for [`Proposer::propose_block`].
pub struct ProposeArgs<Block: BlockT> {
	/// The inherent data to pass to the block production.
	pub inherent_data: InherentData,
	/// The inherent digests to include in the produced block.
	pub inherent_digests: Digest,
	/// Max duration for building the block.
	pub max_duration: time::Duration,
	/// Optional size limit for the produced block.
	///
	/// When set, block production ends before hitting this limit. The limit includes the storage
	/// proof, when proof recording is activated.
	pub block_size_limit: Option<usize>,
	/// Trie nodes that should not be recorded.
	///
	/// Only applies when proof recording is enabled.
	pub ignored_nodes_by_proof_recording: Option<IgnoredNodes<Block::Hash>>,
}

impl<Block: BlockT> Default for ProposeArgs<Block> {
	fn default() -> Self {
		Self {
			inherent_data: Default::default(),
			inherent_digests: Default::default(),
			max_duration: Default::default(),
			block_size_limit: None,
			ignored_nodes_by_proof_recording: None,
		}
	}
}

/// If the block is full we will attempt to push at most
/// this number of transactions before quitting for real.
/// It allows us to increase block utilization.
const MAX_SKIPPED_TRANSACTIONS: usize = 8;

impl<A, Block, C, PR> Proposer<Block, C, A, PR>
where
	A: TransactionPool<Block = Block> + 'static,
	Block: BlockT,
	C: HeaderBackend<Block> + ProvideRuntimeApi<Block> + CallApiAt<Block> + Send + Sync + 'static,
	C::Api: ApiExt<Block> + BlockBuilderApi<Block>,
	PR: ProofRecording,
{
	/// Propose a new block.
	pub async fn propose_block(
		self,
		args: ProposeArgs<Block>,
	) -> Result<Proposal<Block, PR::Proof>, sp_blockchain::Error> {
		let (tx, rx) = oneshot::channel();
		let spawn_handle = self.spawn_handle.clone();

		// Spawn on a new thread, because block production is a blocking operation.
		spawn_handle.spawn_blocking(
			"basic-authorship-proposer",
			None,
			async move {
				let res = self.propose_with(args).await;
				if tx.send(res).is_err() {
					trace!(
						target: LOG_TARGET,
						"Could not send block production result to proposer!"
					);
				}
			}
			.boxed(),
		);

		rx.await?.map_err(Into::into)
	}

	async fn propose_with(
		self,
		ProposeArgs {
			inherent_data,
			inherent_digests,
			max_duration,
			block_size_limit,
			ignored_nodes_by_proof_recording,
		}: ProposeArgs<Block>,
	) -> Result<Proposal<Block, PR::Proof>, sp_blockchain::Error> {
		// leave some time for evaluation and block finalization (10%)
		let deadline = (self.now)() + max_duration - max_duration / 10;
		let block_timer = time::Instant::now();
		let mut block_builder = BlockBuilderBuilder::new(&*self.client)
			.on_parent_block(self.parent_hash)
			.with_parent_block_number(self.parent_number)
			.with_proof_recorder(PR::ENABLED.then(|| {
				ProofRecorder::<Block>::with_ignored_nodes(
					ignored_nodes_by_proof_recording.unwrap_or_default(),
				)
			}))
			.with_inherent_digests(inherent_digests)
			.build()?;

		self.apply_inherents(&mut block_builder, inherent_data)?;

		let mode = block_builder.extrinsic_inclusion_mode();
		let end_reason = match mode {
			ExtrinsicInclusionMode::AllExtrinsics =>
				self.apply_extrinsics(&mut block_builder, deadline, block_size_limit).await?,
			ExtrinsicInclusionMode::OnlyInherents => EndProposingReason::TransactionForbidden,
		};
		let (block, storage_changes, proof) = block_builder.build()?.into_inner();
		let block_took = block_timer.elapsed();

		let proof =
			PR::into_proof(proof).map_err(|e| sp_blockchain::Error::Application(Box::new(e)))?;

		self.print_summary(&block, end_reason, block_took, block_timer.elapsed());
		Ok(Proposal { block, proof, storage_changes })
	}

	/// Apply all inherents to the block.
	fn apply_inherents(
		&self,
		block_builder: &mut sc_block_builder::BlockBuilder<'_, Block, C>,
		inherent_data: InherentData,
	) -> Result<(), sp_blockchain::Error> {
		let create_inherents_start = time::Instant::now();

		let inherent_identifiers = log_enabled!(target: LOG_TARGET, Level::Debug).then(|| {
			inherent_data
				.identifiers()
				.map(|id| String::from_utf8_lossy(id).to_string())
				.collect::<Vec<String>>()
		});

		let inherents = block_builder.create_inherents(inherent_data)?;
		let create_inherents_end = time::Instant::now();

		debug!(target: LOG_TARGET, "apply_inherents: Runtime provided {} inherents. Inherent identifiers present: {:?}", inherents.len(), inherent_identifiers);

		self.metrics.report(|metrics| {
			metrics.create_inherents_time.observe(
				create_inherents_end
					.saturating_duration_since(create_inherents_start)
					.as_secs_f64(),
			);
		});

		for inherent in inherents {
			match block_builder.push(inherent) {
				Err(ApplyExtrinsicFailed(Validity(e))) if e.was_mandatory() => {
					error!(
						"Mandatory inherent extrinsic returned error. Block cannot be produced."
					);
					return Err(ApplyExtrinsicFailed(Validity(e)))
				},
				Err(ApplyExtrinsicFailed(Validity(e))) if e.exhausted_resources() => {
					warn!(
						target: LOG_TARGET,
						"Dropping non-mandatory inherent from overweight block."
					)
				},
				Err(e) => {
					warn!(
						target: LOG_TARGET,
						"Inherent extrinsic returned unexpected error: {}. Dropping.", e
					);
				},
				Ok(_) => {},
			}
		}
		Ok(())
	}

	/// Apply as many extrinsics as possible to the block.
	async fn apply_extrinsics(
		&self,
		block_builder: &mut sc_block_builder::BlockBuilder<'_, Block, C>,
		deadline: time::Instant,
		block_size_limit: Option<usize>,
	) -> Result<EndProposingReason, sp_blockchain::Error> {
		// proceed with transactions
		// We calculate soft deadline used only in case we start skipping transactions.
		let now = (self.now)();
		let left = deadline.saturating_duration_since(now);
		let left_micros: u64 = left.as_micros().saturated_into();
		let soft_deadline =
			now + time::Duration::from_micros(self.soft_deadline_percent.mul_floor(left_micros));
		let mut skipped = 0;
		let mut unqueue_invalid = TxInvalidityReportMap::new();
		let mut limit_hit_reason: Option<EndProposingReason> = None;

		let delay = deadline.saturating_duration_since((self.now)()) / 8;
		let mut pending_iterator =
			self.transaction_pool.ready_at_with_timeout(self.parent_hash, delay).await;

		let block_size_limit = block_size_limit.unwrap_or(self.default_block_size_limit);

		debug!(target: LOG_TARGET, "Attempting to push transactions from the pool at {:?}.", self.parent_hash);
		let mut transaction_pushed = false;

		let end_reason = loop {
			let pending_tx = if let Some(pending_tx) = pending_iterator.next() {
				pending_tx
			} else {
				debug!(
					target: LOG_TARGET,
					"No more transactions, proceeding with proposing."
				);

				break limit_hit_reason.unwrap_or(EndProposingReason::NoMoreTransactions)
			};

			let now = (self.now)();
			if now > deadline {
				debug!(
					target: LOG_TARGET,
					"Consensus deadline reached when pushing block transactions, \
				proceeding with proposing."
				);
				break limit_hit_reason.unwrap_or(EndProposingReason::HitDeadline)
			}

			let pending_tx_data = (**pending_tx.data()).clone();
			let pending_tx_hash = pending_tx.hash().clone();

			let block_size =
				block_builder.estimate_block_size(self.include_proof_in_block_size_estimation);
			if block_size + pending_tx_data.encoded_size() > block_size_limit {
				pending_iterator.report_invalid(&pending_tx);
				limit_hit_reason = Some(EndProposingReason::HitBlockSizeLimit);
				if skipped < MAX_SKIPPED_TRANSACTIONS {
					skipped += 1;
					debug!(
						target: LOG_TARGET,
						"Transaction would overflow the block size limit, \
					 but will try {} more transactions before quitting.",
						MAX_SKIPPED_TRANSACTIONS - skipped,
					);
					continue
				} else if now < soft_deadline {
					debug!(
						target: LOG_TARGET,
						"Transaction would overflow the block size limit, \
					 but we still have time before the soft deadline, so \
					 we will try a bit more."
					);
					continue
				} else {
					debug!(
						target: LOG_TARGET,
						"Reached block size limit, proceeding with proposing."
					);
					break EndProposingReason::HitBlockSizeLimit
				}
			}

			trace!(target: LOG_TARGET, "[{:?}] Pushing to the block.", pending_tx_hash);
			match sc_block_builder::BlockBuilder::push(block_builder, pending_tx_data) {
				Ok(()) => {
					transaction_pushed = true;
					limit_hit_reason = None;
					trace!(target: LOG_TARGET, "[{:?}] Pushed to the block.", pending_tx_hash);
				},
				Err(ApplyExtrinsicFailed(Validity(e))) if e.exhausted_resources() => {
					pending_iterator.report_invalid(&pending_tx);
					limit_hit_reason = Some(EndProposingReason::HitBlockWeightLimit);
					if skipped < MAX_SKIPPED_TRANSACTIONS {
						skipped += 1;
						debug!(target: LOG_TARGET,
							"Block seems full, but will try {} more transactions before quitting.",
							MAX_SKIPPED_TRANSACTIONS - skipped,
						);
					} else if (self.now)() < soft_deadline {
						debug!(target: LOG_TARGET,
							"Block seems full, but we still have time before the soft deadline, \
							 so we will try a bit more before quitting."
						);
					} else {
						debug!(
							target: LOG_TARGET,
							"Reached block weight limit, proceeding with proposing."
						);
						break EndProposingReason::HitBlockWeightLimit
					}
				},
				Err(e) => {
					pending_iterator.report_invalid(&pending_tx);
					debug!(
						target: LOG_TARGET,
						"[{:?}] Invalid transaction: {} at: {}", pending_tx_hash, e, self.parent_hash
					);

					let error_to_report = match e {
						ApplyExtrinsicFailed(Validity(e)) => Some(e),
						_ => None,
					};

					unqueue_invalid.insert(pending_tx_hash, error_to_report);
				},
			}
		};

		if matches!(end_reason, EndProposingReason::HitBlockSizeLimit) && !transaction_pushed {
			warn!(
				target: LOG_TARGET,
				"Hit block size limit of `{}` without including any transaction!", block_size_limit,
			);
		}

		self.transaction_pool
			.report_invalid(Some(self.parent_hash), unqueue_invalid)
			.await;
		Ok(end_reason)
	}

	/// Prints a summary and does telemetry + metrics.
	///
	/// - `block`: The block that was build.
	/// - `end_reason`: Why did we stop producing the block?
	/// - `block_took`: How long did it took to produce the actual block?
	/// - `propose_took`: How long did the entire proposing took?
	fn print_summary(
		&self,
		block: &Block,
		end_reason: EndProposingReason,
		block_took: time::Duration,
		propose_took: time::Duration,
	) {
		let extrinsics = block.extrinsics();
		self.metrics.report(|metrics| {
			metrics.number_of_transactions.set(extrinsics.len() as u64);
			metrics.block_constructed.observe(block_took.as_secs_f64());
			metrics.report_end_proposing_reason(end_reason);
			metrics.create_block_proposal_time.observe(propose_took.as_secs_f64());
		});

		let extrinsics_summary = if extrinsics.is_empty() {
			"no extrinsics".to_string()
		} else {
			format!(
				"extrinsics ({}): [{}]",
				extrinsics.len(),
				extrinsics
					.iter()
					.map(|xt| BlakeTwo256::hash_of(xt).to_string())
					.collect::<Vec<_>>()
					.join(", ")
			)
		};

		if log::log_enabled!(log::Level::Info) {
			info!(
				"Prepared block for proposing at {} ({} ms) hash: {:?}; parent_hash: {}; end: {:?}; extrinsics_count: {}",
				block.header().number(),
				block_took.as_millis(),
				<Block as BlockT>::Hash::from(block.header().hash()),
				block.header().parent_hash(),
				end_reason,
				extrinsics.len()
			)
		} else if log::log_enabled!(log::Level::Trace) {
			trace!(
				"Prepared block for proposing at {} ({} ms) hash: {:?}; parent_hash: {}; end: {:?}; {extrinsics_summary}",
				block.header().number(),
				block_took.as_millis(),
				<Block as BlockT>::Hash::from(block.header().hash()),
				block.header().parent_hash(),
				end_reason
			);
		}

		telemetry!(
			self.telemetry;
			CONSENSUS_INFO;
			"prepared_block_for_proposing";
			"number" => ?block.header().number(),
			"hash" => ?<Block as BlockT>::Hash::from(block.header().hash()),
		);
	}
}

#[cfg(test)]
mod tests {
}
