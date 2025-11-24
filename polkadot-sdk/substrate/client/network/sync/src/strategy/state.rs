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

//! State sync strategy.

use crate::{
	schema::v1::{StateRequest, StateResponse},
	service::network::NetworkServiceHandle,
	strategy::{
		disconnected_peers::DisconnectedPeers,
		state_sync::{ImportResult, StateSync, StateSyncProvider},
		StrategyKey, SyncingAction,
	},
	types::{BadPeer, SyncState, SyncStatus},
	LOG_TARGET,
};
use futures::{channel::oneshot, FutureExt};
use log::{debug, error, trace};
use prost::Message;
use sc_client_api::ProofProvider;
use sc_consensus::{BlockImportError, BlockImportStatus, IncomingBlock};
use sc_network::{IfDisconnected, ProtocolName};
use sc_network_common::sync::message::BlockAnnounce;
use sc_network_types::PeerId;
use sp_consensus::BlockOrigin;
use sp_runtime::{
	traits::{Block as BlockT, Header, NumberFor},
	Justifications, SaturatedConversion,
};
use std::{any::Any, collections::HashMap, sync::Arc};

mod rep {
	use sc_network::ReputationChange as Rep;

	/// Peer response data does not have requested bits.
	pub const BAD_RESPONSE: Rep = Rep::new(-(1 << 12), "Incomplete response");

	/// Reputation change for peers which send us a known bad state.
	pub const BAD_STATE: Rep = Rep::new(-(1 << 29), "Bad state");
}

enum PeerState {
	Available,
	DownloadingState,
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

/// Syncing strategy that downloads and imports a recent state directly.
pub struct StateStrategy<B: BlockT> {
	state_sync: Box<dyn StateSyncProvider<B>>,
	peers: HashMap<PeerId, Peer<B>>,
	disconnected_peers: DisconnectedPeers,
	actions: Vec<SyncingAction<B>>,
	protocol_name: ProtocolName,
	succeeded: bool,
}

impl<B: BlockT> StateStrategy<B> {
	/// Strategy key used by state sync.
	pub const STRATEGY_KEY: StrategyKey = StrategyKey::new("State");

	/// Create a new instance.
	pub fn new<Client>(
		client: Arc<Client>,
		target_header: B::Header,
		target_body: Option<Vec<B::Extrinsic>>,
		target_justifications: Option<Justifications>,
		skip_proof: bool,
		initial_peers: impl Iterator<Item = (PeerId, NumberFor<B>)>,
		protocol_name: ProtocolName,
	) -> Self
	where
		Client: ProofProvider<B> + Send + Sync + 'static,
	{
		let peers = initial_peers
			.map(|(peer_id, best_number)| {
				(peer_id, Peer { best_number, state: PeerState::Available })
			})
			.collect();
		Self {
			state_sync: Box::new(StateSync::new(
				client,
				target_header,
				target_body,
				target_justifications,
				skip_proof,
			)),
			peers,
			disconnected_peers: DisconnectedPeers::new(),
			actions: Vec::new(),
			protocol_name,
			succeeded: false,
		}
	}

	/// Create a new instance with a custom state sync provider.
	///
	/// Note: In most cases, users should use [`StateStrategy::new`].
	/// This method is intended for custom sync strategies and advanced use cases.
	pub fn new_with_provider(
		state_sync_provider: Box<dyn StateSyncProvider<B>>,
		initial_peers: impl Iterator<Item = (PeerId, NumberFor<B>)>,
		protocol_name: ProtocolName,
	) -> Self {
		Self {
			state_sync: state_sync_provider,
			peers: initial_peers
				.map(|(peer_id, best_number)| {
					(peer_id, Peer { best_number, state: PeerState::Available })
				})
				.collect(),
			disconnected_peers: DisconnectedPeers::new(),
			actions: Vec::new(),
			protocol_name,
			succeeded: false,
		}
	}

	/// Notify that a new peer has connected.
	pub fn add_peer(&mut self, peer_id: PeerId, _best_hash: B::Hash, best_number: NumberFor<B>) {
		self.peers.insert(peer_id, Peer { best_number, state: PeerState::Available });
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

	/// Process state response.
	pub fn on_state_response(&mut self, peer_id: &PeerId, response: Vec<u8>) {
		if let Err(bad_peer) = self.on_state_response_inner(peer_id, &response) {
			self.actions.push(SyncingAction::DropPeer(bad_peer));
		}
	}

	fn on_state_response_inner(
		&mut self,
		peer_id: &PeerId,
		response: &[u8],
	) -> Result<(), BadPeer> {
		if let Some(peer) = self.peers.get_mut(&peer_id) {
			peer.state = PeerState::Available;
		}

		let response = match StateResponse::decode(response) {
			Ok(response) => response,
			Err(error) => {
				debug!(
					target: LOG_TARGET,
					"Failed to decode state response from peer {peer_id:?}: {error:?}.",
				);

				return Err(BadPeer(*peer_id, rep::BAD_RESPONSE));
			},
		};

		debug!(
			target: LOG_TARGET,
			"Importing state data from {} with {} keys, {} proof nodes.",
			peer_id,
			response.entries.len(),
			response.proof.len(),
		);

		match self.state_sync.import(response) {
			ImportResult::Import(hash, header, state, body, justifications) => {
				let origin = BlockOrigin::NetworkInitialSync;
				let block = IncomingBlock {
					hash,
					header: Some(header),
					body,
					indexed_body: None,
					justifications,
					origin: None,
					allow_missing_state: true,
					import_existing: true,
					skip_execution: true,
					state: Some(state),
				};
				debug!(target: LOG_TARGET, "State download is complete. Import is queued");
				self.actions.push(SyncingAction::ImportBlocks { origin, blocks: vec![block] });
				Ok(())
			},
			ImportResult::Continue => Ok(()),
			ImportResult::BadResponse => {
				debug!(target: LOG_TARGET, "Bad state data received from {peer_id}");
				Err(BadPeer(*peer_id, rep::BAD_STATE))
			},
		}
	}

	/// A batch of blocks have been processed, with or without errors.
	///
	/// Normally this should be called when target block with state is imported.
	pub fn on_blocks_processed(
		&mut self,
		imported: usize,
		count: usize,
		results: Vec<(Result<BlockImportStatus<NumberFor<B>>, BlockImportError>, B::Hash)>,
	) {
		trace!(target: LOG_TARGET, "State sync: imported {imported} of {count}.");

		let results = results
			.into_iter()
			.filter_map(|(result, hash)| {
				if hash == self.state_sync.target_hash() {
					Some(result)
				} else {
					debug!(
						target: LOG_TARGET,
						"Unexpected block processed: {hash} with result {result:?}.",
					);
					None
				}
			})
			.collect::<Vec<_>>();

		if !results.is_empty() {
			// We processed the target block
			results.iter().filter_map(|result| result.as_ref().err()).for_each(|e| {
				error!(
					target: LOG_TARGET,
					"Failed to import target block with state: {e:?}."
				);
			});
			self.succeeded |= results.into_iter().any(|result| result.is_ok());
			self.actions.push(SyncingAction::Finished);
		}
	}

	/// Produce state request.
	fn state_request(&mut self) -> Option<(PeerId, StateRequest)> {
		if self.state_sync.is_complete() {
			return None
		}

		if self
			.peers
			.values()
			.any(|peer| matches!(peer.state, PeerState::DownloadingState))
		{
			// Only one state request at a time is possible.
			return None
		}

		let peer_id =
			self.schedule_next_peer(PeerState::DownloadingState, self.state_sync.target_number())?;
		let request = self.state_sync.next_request();
		trace!(
			target: LOG_TARGET,
			"New state request to {peer_id}: {request:?}.",
		);
		Some((peer_id, request))
	}

	fn schedule_next_peer(
		&mut self,
		new_state: PeerState,
		min_best_number: NumberFor<B>,
	) -> Option<PeerId> {
		let mut targets: Vec<_> = self.peers.values().map(|p| p.best_number).collect();
		if targets.is_empty() {
			return None
		}
		targets.sort();
		let median = targets[targets.len() / 2];
		let threshold = std::cmp::max(median, min_best_number);
		// Find a random peer that is synced as much as peer majority and is above
		// `min_best_number`.
		for (peer_id, peer) in self.peers.iter_mut() {
			if peer.state.is_available() &&
				peer.best_number >= threshold &&
				self.disconnected_peers.is_peer_available(peer_id)
			{
				peer.state = new_state;
				return Some(*peer_id)
			}
		}
		None
	}

	/// Returns the current sync status.
	pub fn status(&self) -> SyncStatus<B> {
		SyncStatus {
			state: if self.state_sync.is_complete() {
				SyncState::Idle
			} else {
				SyncState::Downloading { target: self.state_sync.target_number() }
			},
			best_seen_block: Some(self.state_sync.target_number()),
			num_peers: self.peers.len().saturated_into(),
			queued_blocks: 0,
			state_sync: Some(self.state_sync.progress()),
			warp_sync: None,
		}
	}

	/// Get actions that should be performed.
	#[must_use]
	pub fn actions(
		&mut self,
		network_service: &NetworkServiceHandle,
	) -> impl Iterator<Item = SyncingAction<B>> {
		let state_request = self.state_request().into_iter().map(|(peer_id, request)| {
			let (tx, rx) = oneshot::channel();

			network_service.start_request(
				peer_id,
				self.protocol_name.clone(),
				request.encode_to_vec(),
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
		self.actions.extend(state_request);

		std::mem::take(&mut self.actions).into_iter()
	}

	/// Check if state sync has succeeded.
	#[must_use]
	pub fn is_succeeded(&self) -> bool {
		self.succeeded
	}
}

#[cfg(test)]
mod test {
}
