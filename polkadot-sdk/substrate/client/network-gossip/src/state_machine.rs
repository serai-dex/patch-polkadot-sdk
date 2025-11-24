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

use crate::{MessageIntent, Network, ValidationResult, Validator, ValidatorContext};

use ahash::AHashSet;
use sc_network_types::PeerId;
use schnellru::{ByLength, LruMap};

use prometheus_endpoint::{register, Counter, PrometheusError, Registry, U64};
use sc_network::{types::ProtocolName, NotificationService};
use sc_network_common::role::ObservedRole;
use sp_runtime::traits::{Block as BlockT, Hash, HashingFor};
use std::{collections::HashMap, iter, sync::Arc, time, time::Instant};

// FIXME: Add additional spam/DoS attack protection: https://github.com/paritytech/substrate/issues/1115
// NOTE: The current value is adjusted based on largest production network deployment (Kusama) and
// the current main gossip user (GRANDPA). Currently there are ~800 validators on Kusama, as such,
// each GRANDPA round should generate ~1600 messages, and we currently keep track of the last 2
// completed rounds and the current live one. That makes it so that at any point we will be holding
// ~4800 live messages.
//
// Assuming that each known message is tracked with a 32 byte hash (common for `Block::Hash`), then
// this cache should take about 256 KB of memory.
const KNOWN_MESSAGES_CACHE_SIZE: u32 = 8192;

const REBROADCAST_INTERVAL: time::Duration = time::Duration::from_millis(750);

pub(crate) const PERIODIC_MAINTENANCE_INTERVAL: time::Duration = time::Duration::from_millis(1100);

mod rep {
	use sc_network::ReputationChange as Rep;
	/// Reputation change when a peer sends us a gossip message that we didn't know about.
	pub const GOSSIP_SUCCESS: Rep = Rep::new(1 << 4, "Successful gossip");
	/// Reputation change when a peer sends us a gossip message that we already knew about.
	pub const DUPLICATE_GOSSIP: Rep = Rep::new(-(1 << 2), "Duplicate gossip");
}

struct PeerConsensus<H> {
	known_messages: AHashSet<H>,
}

/// Topic stream message with sender.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopicNotification {
	/// Message data.
	pub message: Vec<u8>,
	/// Sender if available.
	pub sender: Option<PeerId>,
}

struct MessageEntry<B: BlockT> {
	message_hash: B::Hash,
	topic: B::Hash,
	message: Vec<u8>,
	sender: Option<PeerId>,
}

/// Local implementation of `ValidatorContext`.
struct NetworkContext<'g, 'p, B: BlockT> {
	gossip: &'g mut ConsensusGossip<B>,
	notification_service: &'p mut Box<dyn NotificationService>,
}

impl<'g, 'p, B: BlockT> ValidatorContext<B> for NetworkContext<'g, 'p, B> {
	/// Broadcast all messages with given topic to peers that do not have it yet.
	fn broadcast_topic(&mut self, topic: B::Hash, force: bool) {
		self.gossip.broadcast_topic(self.notification_service, topic, force);
	}

	/// Broadcast a message to all peers that have not received it previously.
	fn broadcast_message(&mut self, topic: B::Hash, message: Vec<u8>, force: bool) {
		self.gossip.multicast(self.notification_service, topic, message, force);
	}

	/// Send addressed message to a peer.
	fn send_message(&mut self, who: &PeerId, message: Vec<u8>) {
		self.notification_service.send_sync_notification(who, message);
	}

	/// Send all messages with given topic to a peer.
	fn send_topic(&mut self, who: &PeerId, topic: B::Hash, force: bool) {
		self.gossip.send_topic(self.notification_service, who, topic, force);
	}
}

fn propagate<'a, B: BlockT, I>(
	notification_service: &mut Box<dyn NotificationService>,
	protocol: ProtocolName,
	messages: I,
	intent: MessageIntent,
	peers: &mut HashMap<PeerId, PeerConsensus<B::Hash>>,
	validator: &Arc<dyn Validator<B>>,
)
// (msg_hash, topic, message)
where
	I: Clone + IntoIterator<Item = (&'a B::Hash, &'a B::Hash, &'a Vec<u8>)>,
{
	let mut message_allowed = validator.message_allowed();

	for (id, ref mut peer) in peers.iter_mut() {
		for (message_hash, topic, message) in messages.clone() {
			let intent = match intent {
				MessageIntent::Broadcast { .. } =>
					if peer.known_messages.contains(message_hash) {
						continue
					} else {
						MessageIntent::Broadcast
					},
				MessageIntent::PeriodicRebroadcast => {
					if peer.known_messages.contains(message_hash) {
						MessageIntent::PeriodicRebroadcast
					} else {
						// peer doesn't know message, so the logic should treat it as an
						// initial broadcast.
						MessageIntent::Broadcast
					}
				},
				other => other,
			};

			if !message_allowed(id, intent, topic, message) {
				continue
			}

			peer.known_messages.insert(*message_hash);

			tracing::trace!(
				target: "gossip",
				to = %id,
				%protocol,
				?message,
				"Propagating message",
			);
			notification_service.send_sync_notification(id, message.clone());
		}
	}
}

/// Consensus network protocol handler. Manages statements and candidate requests.
pub struct ConsensusGossip<B: BlockT> {
	peers: HashMap<PeerId, PeerConsensus<B::Hash>>,
	messages: Vec<MessageEntry<B>>,
	known_messages: LruMap<B::Hash, ()>,
	protocol: ProtocolName,
	validator: Arc<dyn Validator<B>>,
	next_broadcast: Instant,
	metrics: Option<Metrics>,
}

impl<B: BlockT> ConsensusGossip<B> {
	/// Create a new instance using the given validator.
	pub fn new(
		validator: Arc<dyn Validator<B>>,
		protocol: ProtocolName,
		metrics_registry: Option<&Registry>,
	) -> Self {
		let metrics = match metrics_registry.map(Metrics::register) {
			Some(Ok(metrics)) => Some(metrics),
			Some(Err(e)) => {
				tracing::debug!(target: "gossip", "Failed to register metrics: {:?}", e);
				None
			},
			None => None,
		};

		ConsensusGossip {
			peers: HashMap::new(),
			messages: Default::default(),
			known_messages: { LruMap::new(ByLength::new(KNOWN_MESSAGES_CACHE_SIZE)) },
			protocol,
			validator,
			next_broadcast: Instant::now() + REBROADCAST_INTERVAL,
			metrics,
		}
	}

	/// Handle new connected peer.
	pub fn new_peer(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		who: PeerId,
		role: ObservedRole,
	) {
		tracing::trace!(
			target:"gossip",
			%who,
			protocol = %self.protocol,
			?role,
			"Registering peer",
		);
		self.peers.insert(who, PeerConsensus { known_messages: Default::default() });

		let validator = self.validator.clone();
		let mut context = NetworkContext { gossip: self, notification_service };
		validator.new_peer(&mut context, &who, role);
	}

	fn register_message_hashed(
		&mut self,
		message_hash: B::Hash,
		topic: B::Hash,
		message: Vec<u8>,
		sender: Option<PeerId>,
	) {
		if self.known_messages.insert(message_hash, ()) {
			self.messages.push(MessageEntry { message_hash, topic, message, sender });

			if let Some(ref metrics) = self.metrics {
				metrics.registered_messages.inc();
			}
		}
	}

	/// Registers a message without propagating it to any peers. The message
	/// becomes available to new peers or when the service is asked to gossip
	/// the message's topic. No validation is performed on the message, if the
	/// message is already expired it should be dropped on the next garbage
	/// collection.
	pub fn register_message(&mut self, topic: B::Hash, message: Vec<u8>) {
		let message_hash = HashingFor::<B>::hash(&message[..]);
		self.register_message_hashed(message_hash, topic, message, None);
	}

	/// Call when a peer has been disconnected to stop tracking gossip status.
	pub fn peer_disconnected(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		who: PeerId,
	) {
		let validator = self.validator.clone();
		let mut context = NetworkContext { gossip: self, notification_service };
		validator.peer_disconnected(&mut context, &who);
		self.peers.remove(&who);
	}

	/// Perform periodic maintenance
	pub fn tick(&mut self, notification_service: &mut Box<dyn NotificationService>) {
		self.collect_garbage();
		if Instant::now() >= self.next_broadcast {
			self.rebroadcast(notification_service);
			self.next_broadcast = Instant::now() + REBROADCAST_INTERVAL;
		}
	}

	/// Rebroadcast all messages to all peers.
	fn rebroadcast(&mut self, notification_service: &mut Box<dyn NotificationService>) {
		let messages = self
			.messages
			.iter()
			.map(|entry| (&entry.message_hash, &entry.topic, &entry.message));

		propagate(
			notification_service,
			self.protocol.clone(),
			messages,
			MessageIntent::PeriodicRebroadcast,
			&mut self.peers,
			&self.validator,
		);
	}

	/// Broadcast all messages with given topic.
	pub fn broadcast_topic(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		topic: B::Hash,
		force: bool,
	) {
		let messages = self.messages.iter().filter_map(|entry| {
			if entry.topic == topic {
				Some((&entry.message_hash, &entry.topic, &entry.message))
			} else {
				None
			}
		});
		let intent = if force { MessageIntent::ForcedBroadcast } else { MessageIntent::Broadcast };
		propagate(
			notification_service,
			self.protocol.clone(),
			messages,
			intent,
			&mut self.peers,
			&self.validator,
		);
	}

	/// Prune old or no longer relevant consensus messages. Provide a predicate
	/// for pruning, which returns `false` when the items with a given topic should be pruned.
	pub fn collect_garbage(&mut self) {
		let known_messages = &mut self.known_messages;
		let before = self.messages.len();

		let mut message_expired = self.validator.message_expired();
		self.messages.retain(|entry| !message_expired(entry.topic, &entry.message));

		let expired_messages = before - self.messages.len();

		if let Some(ref metrics) = self.metrics {
			metrics.expired_messages.inc_by(expired_messages as u64)
		}

		tracing::trace!(
			target: "gossip",
			protocol = %self.protocol,
			"Cleaned up {} stale messages, {} left ({} known)",
			expired_messages,
			self.messages.len(),
			known_messages.len(),
		);

		for (_, ref mut peer) in self.peers.iter_mut() {
			peer.known_messages.retain(|h| known_messages.get(h).is_some());
		}
	}

	/// Get valid messages received in the past for a topic (might have expired meanwhile).
	pub fn messages_for(&mut self, topic: B::Hash) -> impl Iterator<Item = TopicNotification> + '_ {
		self.messages
			.iter()
			.filter(move |e| e.topic == topic)
			.map(|entry| TopicNotification { message: entry.message.clone(), sender: entry.sender })
	}

	/// Register incoming messages and return the ones that are new and valid (according to a gossip
	/// validator) and should thus be forwarded to the upper layers.
	pub fn on_incoming(
		&mut self,
		network: &mut dyn Network<B>,
		notification_service: &mut Box<dyn NotificationService>,
		who: PeerId,
		messages: Vec<Vec<u8>>,
	) -> Vec<(B::Hash, TopicNotification)> {
		let mut to_forward = vec![];

		if !messages.is_empty() {
			tracing::trace!(
				target: "gossip",
				messages_num = %messages.len(),
				%who,
				protocol = %self.protocol,
				"Received messages from peer",
			);
		}

		for message in messages {
			let message_hash = HashingFor::<B>::hash(&message[..]);

			if self.known_messages.get(&message_hash).is_some() {
				tracing::trace!(
					target: "gossip",
					%who,
					protocol = %self.protocol,
					"Ignored already known message",
				);

				// If the peer already send us the message once, let's report them.
				if self
					.peers
					.get_mut(&who)
					.map_or(false, |p| !p.known_messages.insert(message_hash))
				{
					network.report_peer(who, rep::DUPLICATE_GOSSIP);
				}
				continue
			}

			// validate the message
			let validation = {
				let validator = self.validator.clone();
				let mut context = NetworkContext { gossip: self, notification_service };
				validator.validate(&mut context, &who, &message)
			};

			let (topic, keep) = match validation {
				ValidationResult::ProcessAndKeep(topic) => (topic, true),
				ValidationResult::ProcessAndDiscard(topic) => (topic, false),
				ValidationResult::Discard => {
					tracing::trace!(
						target: "gossip",
						%who,
						protocol = %self.protocol,
						"Discard message from peer",
					);
					continue
				},
			};

			let peer = match self.peers.get_mut(&who) {
				Some(peer) => peer,
				None => {
					tracing::error!(
						target: "gossip",
						%who,
						protocol = %self.protocol,
						"Got message from unregistered peer",
					);
					continue
				},
			};

			network.report_peer(who, rep::GOSSIP_SUCCESS);
			peer.known_messages.insert(message_hash);
			to_forward
				.push((topic, TopicNotification { message: message.clone(), sender: Some(who) }));

			if keep {
				self.register_message_hashed(message_hash, topic, message, Some(who));
			}
		}

		to_forward
	}

	/// Send all messages with given topic to a peer.
	pub fn send_topic(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		who: &PeerId,
		topic: B::Hash,
		force: bool,
	) {
		let mut message_allowed = self.validator.message_allowed();

		if let Some(ref mut peer) = self.peers.get_mut(who) {
			for entry in self.messages.iter().filter(|m| m.topic == topic) {
				let intent =
					if force { MessageIntent::ForcedBroadcast } else { MessageIntent::Broadcast };

				if !force && peer.known_messages.contains(&entry.message_hash) {
					continue
				}

				if !message_allowed(who, intent, &entry.topic, &entry.message) {
					continue
				}

				peer.known_messages.insert(entry.message_hash);

				tracing::trace!(
					target: "gossip",
					to = %who,
					protocol = %self.protocol,
					?entry.message,
					"Sending topic message",
				);
				notification_service.send_sync_notification(who, entry.message.clone());
			}
		}
	}

	/// Multicast a message to all peers.
	pub fn multicast(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		topic: B::Hash,
		message: Vec<u8>,
		force: bool,
	) {
		let message_hash = HashingFor::<B>::hash(&message);
		self.register_message_hashed(message_hash, topic, message.clone(), None);
		let intent = if force { MessageIntent::ForcedBroadcast } else { MessageIntent::Broadcast };
		propagate(
			notification_service,
			self.protocol.clone(),
			iter::once((&message_hash, &topic, &message)),
			intent,
			&mut self.peers,
			&self.validator,
		);
	}

	/// Send addressed message to a peer. The message is not kept or multicast
	/// later on.
	pub fn send_message(
		&mut self,
		notification_service: &mut Box<dyn NotificationService>,
		who: &PeerId,
		message: Vec<u8>,
	) {
		let peer = match self.peers.get_mut(who) {
			None => return,
			Some(peer) => peer,
		};

		let message_hash = HashingFor::<B>::hash(&message);

		tracing::trace!(
			target: "gossip",
			to = %who,
			protocol = %self.protocol,
			?message,
			"Sending direct message",
		);

		peer.known_messages.insert(message_hash);
		notification_service.send_sync_notification(who, message)
	}
}

struct Metrics {
	registered_messages: Counter<U64>,
	expired_messages: Counter<U64>,
}

impl Metrics {
	fn register(registry: &Registry) -> Result<Self, PrometheusError> {
		Ok(Self {
			registered_messages: register(
				Counter::new(
					"substrate_network_gossip_registered_messages_total",
					"Number of registered messages by the gossip service.",
				)?,
				registry,
			)?,
			expired_messages: register(
				Counter::new(
					"substrate_network_gossip_expired_messages_total",
					"Number of expired messages by the gossip service.",
				)?,
				registry,
			)?,
		})
	}
}

#[cfg(test)]
mod tests {
}
