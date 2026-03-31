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

//! GRANDPA block finality proof generation and check.
//!
//! Finality of block B is proved by providing:
//! 1) the justification for the descendant block F;
//! 2) headers sub-chain (B; F] if B != F;
//! 3) proof of GRANDPA::authorities() if the set changes at block F.
//!
//! Since earliest possible justification is returned, the GRANDPA authorities set
//! at the block F is guaranteed to be the same as in the block B (this is because block
//! that enacts new GRANDPA authorities set always comes with justification). It also
//! means that the `set_id` is the same at blocks B and F.
//!
//! Let U be the last finalized block known to caller. If authorities set has changed several
//! times in the (U; F] interval, multiple finality proof fragments are returned (one for each
//! authority set change) and they must be verified in-order.
//!
//! Finality proof provider can choose how to provide finality proof on its own. The incomplete
//! finality proof (that finalizes some block C that is ancestor of the B and descendant
//! of the U) could be returned.

use log::{trace, warn};
use std::sync::Arc;

use codec::{Decode, Encode};
use sc_client_api::backend::Backend;
use sp_blockchain::{Backend as BlockchainBackend, HeaderBackend};
use sp_consensus_grandpa::GRANDPA_ENGINE_ID;
use sp_runtime::{
	generic::BlockId,
	traits::{Block as BlockT, Header as HeaderT, NumberFor, One},
};

use crate::{
	authorities::{AuthoritySetChangeId, AuthoritySetChanges},
	best_justification,
	justification::GrandpaJustification,
	SharedAuthoritySet, LOG_TARGET,
};

const MAX_UNKNOWN_HEADERS: usize = 100_000;

/// Finality proof provider for serving network requests.
#[derive(Clone)]
pub struct FinalityProofProvider<BE, Block: BlockT> {
	backend: Arc<BE>,
	shared_authority_set: Option<SharedAuthoritySet<Block::Hash, NumberFor<Block>>>,
}

impl<B, Block> FinalityProofProvider<B, Block>
where
	Block: BlockT,
	B: Backend<Block>,
{
	/// Create new finality proof provider using:
	///
	/// - backend for accessing blockchain data;
	/// - authority_provider for calling and proving runtime methods.
	/// - shared_authority_set for accessing authority set data
	pub fn new(
		backend: Arc<B>,
		shared_authority_set: Option<SharedAuthoritySet<Block::Hash, NumberFor<Block>>>,
	) -> Self {
		FinalityProofProvider { backend, shared_authority_set }
	}

	/// Create new finality proof provider for the service using:
	///
	/// - backend for accessing blockchain data;
	/// - storage_provider, which is generally a client.
	/// - shared_authority_set for accessing authority set data
	pub fn new_for_service(
		backend: Arc<B>,
		shared_authority_set: Option<SharedAuthoritySet<Block::Hash, NumberFor<Block>>>,
	) -> Arc<Self> {
		Arc::new(Self::new(backend, shared_authority_set))
	}
}

impl<B, Block> FinalityProofProvider<B, Block>
where
	Block: BlockT,
	B: Backend<Block>,
{
	/// Prove finality for the given block number by returning a Justification for the last block of
	/// the authority set in bytes.
	pub fn prove_finality(
		&self,
		block: NumberFor<Block>,
	) -> Result<Option<Vec<u8>>, FinalityProofError> {
		Ok(self.prove_finality_proof(block, true)?.map(|proof| proof.encode()))
	}

	/// Prove finality for the given block number by returning a Justification for the last block of
	/// the authority set.
	///
	/// If `collect_unknown_headers` is true, the finality proof will include all headers from the
	/// requested block until the block the justification refers to.
	pub fn prove_finality_proof(
		&self,
		block: NumberFor<Block>,
		collect_unknown_headers: bool,
	) -> Result<Option<FinalityProof<Block::Header>>, FinalityProofError> {
		let authority_set_changes = if let Some(changes) = self
			.shared_authority_set
			.as_ref()
			.map(SharedAuthoritySet::authority_set_changes)
		{
			changes
		} else {
			return Ok(None);
		};

		prove_finality(&*self.backend, authority_set_changes, block, collect_unknown_headers)
	}
}

/// Finality for block B is proved by providing:
/// 1) the justification for the descendant block F;
/// 2) headers sub-chain (B; F] if B != F;
#[derive(Debug, PartialEq, Encode, Decode, Clone)]
pub struct FinalityProof<Header: HeaderT> {
	/// The hash of block F for which justification is provided.
	pub block: Header::Hash,
	/// Justification of the block F.
	pub justification: Vec<u8>,
	/// The set of headers in the range (B; F] that we believe are unknown to the caller. Ordered.
	pub unknown_headers: Vec<Header>,
}

/// Errors occurring when trying to prove finality
#[derive(Debug, thiserror::Error)]
pub enum FinalityProofError {
	/// The requested block has not yet been finalized.
	#[error("Block not yet finalized")]
	BlockNotYetFinalized,
	/// The requested block is not covered by authority set changes. Likely this means the block is
	/// in the latest authority set, and the subscription API is more appropriate.
	#[error("Block not covered by authority set changes")]
	BlockNotInAuthoritySetChanges,
	/// Errors originating from the client.
	#[error(transparent)]
	Client(#[from] sp_blockchain::Error),
}

/// Prove finality for the given block number by returning a justification for the last block of
/// the authority set of which the given block is part of, or a justification for the latest
/// finalized block if the given block is part of the current authority set.
///
/// If `collect_unknown_headers` is true, the finality proof will include all headers from the
/// requested block until the block the justification refers to.
fn prove_finality<Block, B>(
	backend: &B,
	authority_set_changes: AuthoritySetChanges<NumberFor<Block>>,
	block: NumberFor<Block>,
	collect_unknown_headers: bool,
) -> Result<Option<FinalityProof<Block::Header>>, FinalityProofError>
where
	Block: BlockT,
	B: Backend<Block>,
{
	// Early-return if we are sure that there are no blocks finalized that cover the requested
	// block.
	let finalized_number = backend.blockchain().info().finalized_number;
	if finalized_number < block {
		let err = format!(
			"Requested finality proof for descendant of #{} while we only have finalized #{}.",
			block, finalized_number,
		);
		trace!(target: LOG_TARGET, "{}", &err);
		return Err(FinalityProofError::BlockNotYetFinalized);
	}

	let (justification, just_block) = match authority_set_changes.get_set_id(block) {
		AuthoritySetChangeId::Latest => {
			if let Some(justification) = best_justification(backend)?
				.map(|j: GrandpaJustification<Block>| (j.encode(), j.target().0))
			{
				justification
			} else {
				trace!(
					target: LOG_TARGET,
					"No justification found for the latest finalized block. \
					Returning empty proof.",
				);
				return Ok(None);
			}
		},
		AuthoritySetChangeId::Set(_, last_block_for_set) => {
			let last_block_for_set_id = backend
				.blockchain()
				.expect_block_hash_from_id(&BlockId::Number(last_block_for_set))?;
			let justification = if let Some(grandpa_justification) = backend
				.blockchain()
				.justifications(last_block_for_set_id)?
				.and_then(|justifications| justifications.into_justification(GRANDPA_ENGINE_ID))
			{
				grandpa_justification
			} else {
				trace!(
					target: LOG_TARGET,
					"No justification found when making finality proof for {}. \
					Returning empty proof.",
					block,
				);
				return Ok(None);
			};
			(justification, last_block_for_set)
		},
		AuthoritySetChangeId::Unknown => {
			warn!(
				target: LOG_TARGET,
				"AuthoritySetChanges does not cover the requested block #{} due to missing data. \
				 You need to resync to populate AuthoritySetChanges properly.",
				block,
			);
			return Err(FinalityProofError::BlockNotInAuthoritySetChanges);
		},
	};

	let mut headers = Vec::new();
	if collect_unknown_headers {
		// Collect all headers from the requested block until the last block of the set
		let mut current = block + One::one();
		loop {
			if current > just_block || headers.len() >= MAX_UNKNOWN_HEADERS {
				break;
			}
			let hash = backend.blockchain().expect_block_hash_from_id(&BlockId::Number(current))?;
			headers.push(backend.blockchain().expect_header(hash)?);
			current += One::one();
		}
	};

	Ok(Some(FinalityProof {
		block: backend.blockchain().expect_block_hash_from_id(&BlockId::Number(just_block))?,
		justification,
		unknown_headers: headers,
	}))
}

#[cfg(test)]
mod tests {
}
