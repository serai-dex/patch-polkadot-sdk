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

//! Warp syncing strategy. Bootstraps chain by downloading warp proofs and state.

use sc_consensus::IncomingBlock;
use sp_consensus::BlockOrigin;

use crate::{
	block_relay_protocol::{BlockDownloader, BlockResponseError},
	service::network::NetworkServiceHandle,
	strategy::{
		chain_sync::validate_blocks, disconnected_peers::DisconnectedPeers, StrategyKey,
		SyncingAction,
	},
	types::{BadPeer, SyncState, SyncStatus},
	LOG_TARGET,
};
use codec::{Decode, Encode};
use futures::{channel::oneshot, FutureExt};
use log::{debug, error, trace, warn};
use sc_network::{IfDisconnected, ProtocolName};
use sc_network_common::sync::message::{
	BlockAnnounce, BlockAttributes, BlockData, BlockRequest, Direction, FromBlock,
};
use sc_network_types::PeerId;
use sp_blockchain::HeaderBackend;
use sp_runtime::{
	traits::{Block as BlockT, Header, NumberFor, Zero},
	Justifications, SaturatedConversion,
};
use std::{any::Any, collections::HashMap, fmt, sync::Arc};

/// Number of peers that need to be connected before warp sync is started.
const MIN_PEERS_TO_START_WARP_SYNC: usize = 3;

/// Scale-encoded warp sync proof response.
pub struct EncodedProof(pub Vec<u8>);

/// Warp sync request
#[derive(Encode, Decode, Debug, Clone)]
pub struct WarpProofRequest<B: BlockT> {
	/// Start collecting proofs from this block.
	pub begin: B::Hash,
}

/// Verifier for warp sync proofs. Each verifier operates in a specific context.
pub trait Verifier<Block: BlockT>: Send + Sync {
	/// Verify a warp sync proof.
	fn verify(
		&mut self,
		proof: &EncodedProof,
	) -> Result<VerificationResult<Block>, Box<dyn std::error::Error + Send + Sync>>;
	/// Hash to be used as the starting point for the next proof request.
	fn next_proof_context(&self) -> Block::Hash;
	/// Get status text for progress reporting
	fn status(&self) -> Option<String>;
}

/// Proof verification result.
pub enum VerificationResult<Block: BlockT> {
	/// Proof is valid, but the target was not reached.
	Partial(Vec<(Block::Header, Justifications)>),
	/// Target finality is proved.
	Complete(Block::Header, Vec<(Block::Header, Justifications)>),
}

/// Warp sync backend. Handles retrieving and verifying warp sync proofs.
pub trait WarpSyncProvider<Block: BlockT>: Send + Sync {
	/// Generate proof starting at given block hash. The proof is accumulated until maximum proof
	/// size is reached.
	fn generate(
		&self,
		start: Block::Hash,
	) -> Result<EncodedProof, Box<dyn std::error::Error + Send + Sync>>;
	/// Create a verifier for warp sync proofs.
	fn create_verifier(&self) -> Box<dyn Verifier<Block>>;
}

mod rep {
	use sc_network::ReputationChange as Rep;

	/// Unexpected response received form a peer
	pub const UNEXPECTED_RESPONSE: Rep = Rep::new(-(1 << 29), "Unexpected response");

	/// Peer provided invalid warp proof data
	pub const BAD_WARP_PROOF: Rep = Rep::new(-(1 << 29), "Bad warp proof");

	/// Peer did not provide us with advertised block data.
	pub const NO_BLOCK: Rep = Rep::new(-(1 << 29), "No requested block data");

	/// Reputation change for peers which send us non-requested block data.
	pub const NOT_REQUESTED: Rep = Rep::new(-(1 << 29), "Not requested block data");

	/// Reputation change for peers which send us a block which we fail to verify.
	pub const VERIFICATION_FAIL: Rep = Rep::new(-(1 << 29), "Block verification failed");

	/// We received a message that failed to decode.
	pub const BAD_MESSAGE: Rep = Rep::new(-(1 << 12), "Bad message");
}

/// Reported warp sync phase.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum WarpSyncPhase<Block: BlockT> {
	/// Waiting for peers to connect.
	AwaitingPeers { required_peers: usize },
	/// Downloading and verifying warp proofs.
	DownloadingWarpProofs,
	/// Downloading target block.
	DownloadingTargetBlock,
	/// Downloading state data.
	DownloadingState,
	/// Importing state.
	ImportingState,
	/// Downloading block history.
	DownloadingBlocks(NumberFor<Block>),
	/// Warp sync is complete.
	Complete,
}

impl<Block: BlockT> fmt::Display for WarpSyncPhase<Block> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::AwaitingPeers { required_peers } => {
				write!(f, "Waiting for {required_peers} peers to be connected")
			},
			Self::DownloadingWarpProofs => write!(f, "Downloading finality proofs"),
			Self::DownloadingTargetBlock => write!(f, "Downloading target block"),
			Self::DownloadingState => write!(f, "Downloading state"),
			Self::ImportingState => write!(f, "Importing state"),
			Self::DownloadingBlocks(n) => write!(f, "Downloading block history (#{})", n),
			Self::Complete => write!(f, "Warp sync is complete"),
		}
	}
}

/// Reported warp sync progress.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WarpSyncProgress<Block: BlockT> {
	/// Estimated download percentage.
	pub phase: WarpSyncPhase<Block>,
	/// Total bytes downloaded so far.
	pub total_bytes: u64,
	/// Optional status text from the verifier.
	pub status: Option<String>,
}

/// Warp sync configuration as accepted by [`WarpSync`].
pub enum WarpSyncConfig<Block: BlockT> {
	/// Standard warp sync for the chain.
	WithProvider(Arc<dyn WarpSyncProvider<Block>>),
	/// Skip downloading proofs and use provided header of the state that should be downloaded.
	///
	/// It is expected that the header provider ensures that the header is trusted.
	WithTarget(<Block as BlockT>::Header),
}

/// Warp sync phase used by warp sync state machine.
enum Phase<B: BlockT> {
	/// Waiting for enough peers to connect.
	WaitingForPeers { warp_sync_provider: Arc<dyn WarpSyncProvider<B>> },
	/// Downloading warp proofs.
	WarpProof { verifier: Box<dyn Verifier<B>> },
	/// Downloading target block.
	TargetBlock(B::Header),
	/// Warp sync is complete.
	Complete,
}

enum PeerState {
	Available,
	DownloadingProofs,
	DownloadingTargetBlock,
}

impl PeerState {
	fn is_available(&self) -> bool {
		matches!(self, PeerState::Available)
	}
}

struct Peer<B: BlockT> {
	best_number: NumberFor<B>,
	state: PeerState,
}

pub struct WarpSyncResult<B: BlockT> {
	pub target_header: B::Header,
	pub target_body: Option<Vec<B::Extrinsic>>,
	pub target_justifications: Option<Justifications>,
}

/// Warp sync state machine. Accumulates warp proofs and state.
pub struct WarpSync<B: BlockT> {
	phase: Phase<B>,
	total_proof_bytes: u64,
	total_state_bytes: u64,
	peers: HashMap<PeerId, Peer<B>>,
	disconnected_peers: DisconnectedPeers,
	protocol_name: Option<ProtocolName>,
	block_downloader: Arc<dyn BlockDownloader<B>>,
	actions: Vec<SyncingAction<B>>,
	result: Option<WarpSyncResult<B>>,
	/// Number of peers that need to be connected before warp sync is started.
	min_peers_to_start_warp_sync: usize,
}

impl<B> WarpSync<B>
where
	B: BlockT,
{
	/// Strategy key used by warp sync.
	pub const STRATEGY_KEY: StrategyKey = StrategyKey::new("Warp");

	/// Create a new instance. When passing a warp sync provider we will be checking for proof and
	/// authorities. Alternatively we can pass a target block when we want to skip downloading
	/// proofs, in this case we will continue polling until the target block is known.
	pub fn new<Client>(
		client: Arc<Client>,
		warp_sync_config: WarpSyncConfig<B>,
		protocol_name: Option<ProtocolName>,
		block_downloader: Arc<dyn BlockDownloader<B>>,
		min_peers_to_start_warp_sync: Option<usize>,
	) -> Self
	where
		Client: HeaderBackend<B> + 'static,
	{
		let min_peers_to_start_warp_sync =
			min_peers_to_start_warp_sync.unwrap_or(MIN_PEERS_TO_START_WARP_SYNC);
		if client.info().finalized_state.is_some() {
			error!(
				target: LOG_TARGET,
				"Can't use warp sync mode with a partially synced database. Reverting to full sync mode."
			);
			return Self {
				phase: Phase::Complete,
				total_proof_bytes: 0,
				total_state_bytes: 0,
				peers: HashMap::new(),
				disconnected_peers: DisconnectedPeers::new(),
				protocol_name,
				block_downloader,
				actions: vec![SyncingAction::Finished],
				result: None,
				min_peers_to_start_warp_sync,
			};
		}

		let phase = match warp_sync_config {
			WarpSyncConfig::WithProvider(warp_sync_provider) => {
				Phase::WaitingForPeers { warp_sync_provider }
			},
			WarpSyncConfig::WithTarget(target_header) => Phase::TargetBlock(target_header),
		};

		Self {
			phase,
			total_proof_bytes: 0,
			total_state_bytes: 0,
			peers: HashMap::new(),
			disconnected_peers: DisconnectedPeers::new(),
			protocol_name,
			block_downloader,
			actions: Vec::new(),
			result: None,
			min_peers_to_start_warp_sync,
		}
	}

	/// Notify that a new peer has connected.
	pub fn add_peer(&mut self, peer_id: PeerId, _best_hash: B::Hash, best_number: NumberFor<B>) {
		self.peers.insert(peer_id, Peer { best_number, state: PeerState::Available });

		self.try_to_start_warp_sync();
	}

	/// Notify that a peer has disconnected.
	pub fn remove_peer(&mut self, peer_id: &PeerId) {
		if let Some(state) = self.peers.remove(peer_id) {
			if !state.state.is_available() {
				if let Some(bad_peer) =
					self.disconnected_peers.on_disconnect_during_request(*peer_id)
				{
					self.actions.push(SyncingAction::DropPeer(bad_peer));
				}
			}
		}
	}

	/// Submit a validated block announcement.
	///
	/// Returns new best hash & best number of the peer if they are updated.
	#[must_use]
	pub fn on_validated_block_announce(
		&mut self,
		is_best: bool,
		peer_id: PeerId,
		announce: &BlockAnnounce<B::Header>,
	) -> Option<(B::Hash, NumberFor<B>)> {
		is_best.then(|| {
			let best_number = *announce.header.number();
			let best_hash = announce.header.hash();
			if let Some(ref mut peer) = self.peers.get_mut(&peer_id) {
				peer.best_number = best_number;
			}
			// Let `SyncingEngine` know that we should update the peer info.
			(best_hash, best_number)
		})
	}

	/// Start warp sync as soon as we have enough peers.
	fn try_to_start_warp_sync(&mut self) {
		let Phase::WaitingForPeers { warp_sync_provider } = &self.phase else { return };

		if self.peers.len() < self.min_peers_to_start_warp_sync {
			return;
		}

		let verifier = warp_sync_provider.create_verifier();
		self.phase = Phase::WarpProof { verifier };
		debug!(target: LOG_TARGET, "Started warp sync with {} peers.", self.peers.len());
	}

	pub fn on_generic_response(
		&mut self,
		peer_id: &PeerId,
		protocol_name: ProtocolName,
		response: Box<dyn Any + Send>,
	) {
		if &protocol_name == self.block_downloader.protocol_name() {
			let Ok(response) = response
				.downcast::<(BlockRequest<B>, Result<Vec<BlockData<B>>, BlockResponseError>)>()
			else {
				warn!(target: LOG_TARGET, "Failed to downcast block response");
				debug_assert!(false);
				return;
			};

			let (request, response) = *response;
			let blocks = match response {
				Ok(blocks) => blocks,
				Err(BlockResponseError::DecodeFailed(e)) => {
					debug!(
						target: LOG_TARGET,
						"Failed to decode block response from peer {:?}: {:?}.",
						peer_id,
						e
					);
					self.actions.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::BAD_MESSAGE)));
					return;
				},
				Err(BlockResponseError::ExtractionFailed(e)) => {
					debug!(
						target: LOG_TARGET,
						"Failed to extract blocks from peer response {:?}: {:?}.",
						peer_id,
						e
					);
					self.actions.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::BAD_MESSAGE)));
					return;
				},
			};

			self.on_block_response(*peer_id, request, blocks);
		} else {
			let Ok(response) = response.downcast::<Vec<u8>>() else {
				warn!(target: LOG_TARGET, "Failed to downcast warp sync response");
				debug_assert!(false);
				return;
			};

			self.on_warp_proof_response(peer_id, EncodedProof(*response));
		}
	}

	/// Process warp proof response.
	pub fn on_warp_proof_response(&mut self, peer_id: &PeerId, response: EncodedProof) {
		if let Some(peer) = self.peers.get_mut(peer_id) {
			peer.state = PeerState::Available;
		}

		let Phase::WarpProof { verifier } = &mut self.phase else {
			debug!(target: LOG_TARGET, "Unexpected warp proof response");
			self.actions
				.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::UNEXPECTED_RESPONSE)));
			return;
		};

		let proof_to_incoming_block =
			|(header, justifications): (B::Header, Justifications)| -> IncomingBlock<B> {
				IncomingBlock {
					hash: header.hash(),
					header: Some(header),
					body: None,
					indexed_body: None,
					justifications: Some(justifications),
					origin: Some(*peer_id),
					// We are still in warp sync, so we don't have the state. This means
					// we also can't execute the block.
					allow_missing_state: true,
					skip_execution: true,
					// Shouldn't already exist in the database.
					import_existing: false,
					state: None,
				}
			};

		match verifier.verify(&response) {
			Err(e) => {
				debug!(target: LOG_TARGET, "Bad warp proof response: {}", e);
				self.actions
					.push(SyncingAction::DropPeer(BadPeer(*peer_id, rep::BAD_WARP_PROOF)))
			},
			Ok(VerificationResult::Partial(proofs)) => {
				debug!(target: LOG_TARGET, "Verified partial proof");
				self.total_proof_bytes += response.0.len() as u64;
				self.actions.push(SyncingAction::ImportBlocks {
					origin: BlockOrigin::WarpSync,
					blocks: proofs.into_iter().map(proof_to_incoming_block).collect(),
				});
			},
			Ok(VerificationResult::Complete(header, proofs)) => {
				debug!(
					target: LOG_TARGET,
					"Verified complete proof. Continuing with target block download: {} ({}).",
					header.hash(),
					header.number(),
				);
				self.total_proof_bytes += response.0.len() as u64;
				self.phase = Phase::TargetBlock(header.clone());
				let incoming_blocks: Vec<_> = proofs
					.into_iter()
					.map(proof_to_incoming_block)
					.filter(|i| {
						// We need target block with state and warp sync does not provide this.
						// That's why we filter out target block here, otherwise oncoming state sync
						// (which comes after warp sync) will abort because target block is already
						// imported.
						if header.number() != i.header.as_ref().unwrap().number() {
							true
						} else {
							log::trace!(
								target: LOG_TARGET,
								"Filtered out target block: {} ({})",
								header.hash(),
								header.number()
							);
							false
						}
					})
					.collect();
				self.actions.push(SyncingAction::ImportBlocks {
					origin: BlockOrigin::WarpSync,
					blocks: incoming_blocks,
				});
			},
		}
	}

	/// Process (target) block response.
	pub fn on_block_response(
		&mut self,
		peer_id: PeerId,
		request: BlockRequest<B>,
		blocks: Vec<BlockData<B>>,
	) {
		if let Err(bad_peer) = self.on_block_response_inner(peer_id, request, blocks) {
			self.actions.push(SyncingAction::DropPeer(bad_peer));
		}
	}

	fn on_block_response_inner(
		&mut self,
		peer_id: PeerId,
		request: BlockRequest<B>,
		mut blocks: Vec<BlockData<B>>,
	) -> Result<(), BadPeer> {
		if let Some(peer) = self.peers.get_mut(&peer_id) {
			peer.state = PeerState::Available;
		}

		let Phase::TargetBlock(header) = &mut self.phase else {
			debug!(target: LOG_TARGET, "Unexpected target block response from {peer_id}");
			return Err(BadPeer(peer_id, rep::UNEXPECTED_RESPONSE));
		};

		if blocks.is_empty() {
			debug!(
				target: LOG_TARGET,
				"Downloading target block failed: empty block response from {peer_id}",
			);
			return Err(BadPeer(peer_id, rep::NO_BLOCK));
		}

		if blocks.len() > 1 {
			debug!(
				target: LOG_TARGET,
				"Too many blocks ({}) in warp target block response from {peer_id}",
				blocks.len(),
			);
			return Err(BadPeer(peer_id, rep::NOT_REQUESTED));
		}

		validate_blocks::<B>(&blocks, &peer_id, Some(request))?;

		let block = blocks.pop().expect("`blocks` len checked above");

		let Some(block_header) = &block.header else {
			debug!(
				target: LOG_TARGET,
				"Downloading target block failed: missing header in response from {peer_id}.",
			);
			return Err(BadPeer(peer_id, rep::VERIFICATION_FAIL));
		};

		if block_header != header {
			debug!(
				target: LOG_TARGET,
				"Downloading target block failed: different header in response from {peer_id}.",
			);
			return Err(BadPeer(peer_id, rep::VERIFICATION_FAIL));
		}

		if block.body.is_none() {
			debug!(
				target: LOG_TARGET,
				"Downloading target block failed: missing body in response from {peer_id}.",
			);
			return Err(BadPeer(peer_id, rep::VERIFICATION_FAIL));
		}

		self.result = Some(WarpSyncResult {
			target_header: header.clone(),
			target_body: block.body,
			target_justifications: block.justifications,
		});
		self.phase = Phase::Complete;
		self.actions.push(SyncingAction::Finished);
		Ok(())
	}

	/// Reserve a peer for a request assigning `new_state`.
	fn schedule_next_peer(
		&mut self,
		new_state: PeerState,
		min_best_number: Option<NumberFor<B>>,
	) -> Option<PeerId> {
		let mut targets: Vec<_> = self.peers.values().map(|p| p.best_number).collect();
		if targets.is_empty() {
			return None;
		}
		targets.sort();
		let median = targets[targets.len() / 2];
		let threshold = std::cmp::max(median, min_best_number.unwrap_or(Zero::zero()));
		// Find a random peer that is synced as much as peer majority and is above
		// `min_best_number`.
		for (peer_id, peer) in self.peers.iter_mut() {
			if peer.state.is_available() &&
				peer.best_number >= threshold &&
				self.disconnected_peers.is_peer_available(peer_id)
			{
				peer.state = new_state;
				return Some(*peer_id);
			}
		}
		None
	}

	/// Produce warp proof request.
	fn warp_proof_request(&mut self) -> Option<(PeerId, ProtocolName, WarpProofRequest<B>)> {
		let Phase::WarpProof { verifier } = &self.phase else { return None };

		// Copy verifier context early to cut the borrowing tie.
		let begin = verifier.next_proof_context();

		if self
			.peers
			.values()
			.any(|peer| matches!(peer.state, PeerState::DownloadingProofs))
		{
			// Only one warp proof request at a time is possible.
			return None;
		}

		let peer_id = self.schedule_next_peer(PeerState::DownloadingProofs, None)?;
		trace!(target: LOG_TARGET, "New WarpProofRequest to {peer_id}, begin hash: {begin}.");

		let request = WarpProofRequest { begin };

		let Some(protocol_name) = self.protocol_name.clone() else {
			warn!(
				target: LOG_TARGET,
				"Trying to send warp sync request when no protocol is configured {request:?}",
			);
			return None;
		};

		Some((peer_id, protocol_name, request))
	}

	/// Produce target block request.
	fn target_block_request(&mut self) -> Option<(PeerId, BlockRequest<B>)> {
		let Phase::TargetBlock(target_header) = &self.phase else { return None };

		if self
			.peers
			.values()
			.any(|peer| matches!(peer.state, PeerState::DownloadingTargetBlock))
		{
			// Only one target block request at a time is possible.
			return None;
		}

		// Cut the borrowing tie.
		let target_hash = target_header.hash();
		let target_number = *target_header.number();

		let peer_id =
			self.schedule_next_peer(PeerState::DownloadingTargetBlock, Some(target_number))?;

		trace!(
			target: LOG_TARGET,
			"New target block request to {peer_id}, target: {} ({}).",
			target_hash,
			target_number,
		);

		Some((
			peer_id,
			BlockRequest::<B> {
				id: 0,
				fields: BlockAttributes::HEADER |
					BlockAttributes::BODY |
					BlockAttributes::JUSTIFICATION,
				from: FromBlock::Hash(target_hash),
				direction: Direction::Ascending,
				max: Some(1),
			},
		))
	}

	/// Returns warp sync estimated progress (stage, bytes received).
	pub fn progress(&self) -> WarpSyncProgress<B> {
		match &self.phase {
			Phase::WaitingForPeers { .. } => WarpSyncProgress {
				phase: WarpSyncPhase::AwaitingPeers {
					required_peers: self.min_peers_to_start_warp_sync,
				},
				total_bytes: self.total_proof_bytes,
				status: None,
			},
			Phase::WarpProof { verifier } => WarpSyncProgress {
				phase: WarpSyncPhase::DownloadingWarpProofs,
				total_bytes: self.total_proof_bytes,
				status: verifier.status(),
			},
			Phase::TargetBlock(_) => WarpSyncProgress {
				phase: WarpSyncPhase::DownloadingTargetBlock,
				total_bytes: self.total_proof_bytes,
				status: None,
			},
			Phase::Complete => WarpSyncProgress {
				phase: WarpSyncPhase::Complete,
				total_bytes: self.total_proof_bytes + self.total_state_bytes,
				status: None,
			},
		}
	}

	/// Get the number of peers known to warp sync.
	pub fn num_peers(&self) -> usize {
		self.peers.len()
	}

	/// Returns the current sync status.
	pub fn status(&self) -> SyncStatus<B> {
		SyncStatus {
			state: match &self.phase {
				Phase::WaitingForPeers { .. } => SyncState::Downloading { target: Zero::zero() },
				Phase::WarpProof { .. } => SyncState::Downloading { target: Zero::zero() },
				Phase::TargetBlock(header) => SyncState::Downloading { target: *header.number() },
				Phase::Complete => SyncState::Idle,
			},
			best_seen_block: match &self.phase {
				Phase::WaitingForPeers { .. } => None,
				Phase::WarpProof { .. } => None,
				Phase::TargetBlock(header) => Some(*header.number()),
				Phase::Complete => None,
			},
			num_peers: self.peers.len().saturated_into(),
			queued_blocks: 0,
			state_sync: None,
			warp_sync: Some(self.progress()),
		}
	}

	/// Get actions that should be performed by the owner on [`WarpSync`]'s behalf
	#[must_use]
	pub fn actions(
		&mut self,
		network_service: &NetworkServiceHandle,
	) -> impl Iterator<Item = SyncingAction<B>> {
		let warp_proof_request =
			self.warp_proof_request().into_iter().map(|(peer_id, protocol_name, request)| {
				trace!(
					target: LOG_TARGET,
					"Created `WarpProofRequest` to {}, request: {:?}.",
					peer_id,
					request,
				);

				let (tx, rx) = oneshot::channel();

				network_service.start_request(
					peer_id,
					protocol_name,
					request.encode(),
					tx,
					IfDisconnected::ImmediateError,
				);

				SyncingAction::StartRequest {
					peer_id,
					key: Self::STRATEGY_KEY,
					request: async move {
						Ok(rx.await?.and_then(|(response, protocol_name)| {
							Ok((Box::new(response) as Box<dyn Any + Send>, protocol_name))
						}))
					}
					.boxed(),
					remove_obsolete: false,
				}
			});
		self.actions.extend(warp_proof_request);

		let target_block_request =
			self.target_block_request().into_iter().map(|(peer_id, request)| {
				let downloader = self.block_downloader.clone();

				SyncingAction::StartRequest {
					peer_id,
					key: Self::STRATEGY_KEY,
					request: async move {
						Ok(downloader.download_blocks(peer_id, request.clone()).await?.and_then(
							|(response, protocol_name)| {
								let decoded_response =
									downloader.block_response_into_blocks(&request, response);
								let result =
									Box::new((request, decoded_response)) as Box<dyn Any + Send>;
								Ok((result, protocol_name))
							},
						))
					}
					.boxed(),
					// Sending block request implies dropping obsolete pending response as we are
					// not interested in it anymore.
					remove_obsolete: true,
				}
			});
		self.actions.extend(target_block_request);

		std::mem::take(&mut self.actions).into_iter()
	}

	/// Take the result of finished warp sync, returning `None` if the sync was unsuccessful.
	#[must_use]
	pub fn take_result(&mut self) -> Option<WarpSyncResult<B>> {
		self.result.take()
	}
}

#[cfg(test)]
mod test {
}
