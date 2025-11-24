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

//! Notification pinning related logic.
//!
//! This file contains a worker that should be started when a new client instance is created.
//! The goal is to avoid pruning of blocks that have active notifications in the node. Every
//! recipient of notifications should receive the chance to act upon them. In addition, notification
//! listeners can hold onto a [`sc_client_api::UnpinHandle`] to keep a block pinned. Once the handle
//! is dropped, a message is sent and the worker unpins the respective block.
use std::{
	marker::PhantomData,
	sync::{Arc, Weak},
};

use futures::StreamExt;
use sc_client_api::{Backend, UnpinWorkerMessage};

use sc_utils::mpsc::TracingUnboundedReceiver;
use schnellru::Limiter;
use sp_runtime::traits::Block as BlockT;

const LOG_TARGET: &str = "db::notification_pinning";
const NOTIFICATION_PINNING_LIMIT: usize = 1024;

/// A limiter which automatically unpins blocks that leave the data structure.
#[derive(Clone, Debug)]
struct UnpinningByLengthLimiter<Block: BlockT, B: Backend<Block>> {
	max_length: usize,
	backend: Weak<B>,
	_phantom: PhantomData<Block>,
}

impl<Block: BlockT, B: Backend<Block>> UnpinningByLengthLimiter<Block, B> {
	/// Creates a new length limiter with a given `max_length`.
	pub fn new(max_length: usize, backend: Weak<B>) -> UnpinningByLengthLimiter<Block, B> {
		UnpinningByLengthLimiter { max_length, backend, _phantom: PhantomData::<Block>::default() }
	}
}

impl<Block: BlockT, B: Backend<Block>> Limiter<Block::Hash, u32>
	for UnpinningByLengthLimiter<Block, B>
{
	type KeyToInsert<'a> = Block::Hash;
	type LinkType = usize;

	fn is_over_the_limit(&self, length: usize) -> bool {
		length > self.max_length
	}

	fn on_insert(
		&mut self,
		_length: usize,
		key: Self::KeyToInsert<'_>,
		value: u32,
	) -> Option<(Block::Hash, u32)> {
		log::debug!(target: LOG_TARGET, "Pinning block based on notification. hash = {key}");
		if self.max_length > 0 {
			Some((key, value))
		} else {
			None
		}
	}

	fn on_replace(
		&mut self,
		_length: usize,
		_old_key: &mut Block::Hash,
		_new_key: Block::Hash,
		_old_value: &mut u32,
		_new_value: &mut u32,
	) -> bool {
		true
	}

	fn on_removed(&mut self, key: &mut Block::Hash, references: &mut u32) {
		// If reference count was larger than 0 on removal,
		// the item was removed due to capacity limitations.
		// Since the cache should be large enough for pinned items,
		// we want to know about these evictions.
		if *references > 0 {
			log::warn!(
				target: LOG_TARGET,
				"Notification block pinning limit reached. Unpinning block with hash = {key:?}"
			);
			if let Some(backend) = self.backend.upgrade() {
				(0..*references).for_each(|_| backend.unpin_block(*key));
			}
		} else {
			log::trace!(
				target: LOG_TARGET,
				"Unpinned block. hash = {key:?}",
			)
		}
	}

	fn on_cleared(&mut self) {}

	fn on_grow(&mut self, _new_memory_usage: usize) -> bool {
		true
	}
}

/// Worker for the handling of notification pinning.
///
/// It receives messages from a receiver and pins/unpins based on the incoming messages.
/// All notification related unpinning should go through this worker. If the maximum number of
/// notification pins is reached, the block from the oldest notification is unpinned.
pub struct NotificationPinningWorker<Block: BlockT, Back: Backend<Block>> {
	unpin_message_rx: TracingUnboundedReceiver<UnpinWorkerMessage<Block>>,
	task_backend: Weak<Back>,
	pinned_blocks: schnellru::LruMap<Block::Hash, u32, UnpinningByLengthLimiter<Block, Back>>,
}

impl<Block: BlockT, Back: Backend<Block>> NotificationPinningWorker<Block, Back> {
	/// Creates a new `NotificationPinningWorker`.
	pub fn new(
		unpin_message_rx: TracingUnboundedReceiver<UnpinWorkerMessage<Block>>,
		task_backend: Arc<Back>,
	) -> Self {
		let pinned_blocks =
			schnellru::LruMap::<Block::Hash, u32, UnpinningByLengthLimiter<Block, Back>>::new(
				UnpinningByLengthLimiter::new(
					NOTIFICATION_PINNING_LIMIT,
					Arc::downgrade(&task_backend),
				),
			);
		Self { unpin_message_rx, task_backend: Arc::downgrade(&task_backend), pinned_blocks }
	}

	fn handle_announce_message(&mut self, hash: Block::Hash) {
		if let Some(entry) = self.pinned_blocks.get_or_insert(hash, Default::default) {
			*entry = *entry + 1;
		}
	}

	fn handle_unpin_message(&mut self, hash: Block::Hash) -> Result<(), ()> {
		if let Some(refcount) = self.pinned_blocks.peek_mut(&hash) {
			*refcount = *refcount - 1;
			if *refcount == 0 {
				self.pinned_blocks.remove(&hash);
			}
			if let Some(backend) = self.task_backend.upgrade() {
				log::debug!(target: LOG_TARGET, "Reducing pinning refcount for block hash = {hash:?}");
				backend.unpin_block(hash);
			} else {
				log::debug!(target: LOG_TARGET, "Terminating unpin-worker, backend reference was dropped.");
				return Err(())
			}
		} else {
			log::debug!(target: LOG_TARGET, "Received unpin message for already unpinned block. hash = {hash:?}");
		}
		Ok(())
	}

	/// Start working on the received messages.
	///
	/// The worker maintains a map which keeps track of the pinned blocks and their reference count.
	/// Depending upon the received message, it acts to pin/unpin the block.
	pub async fn run(mut self) {
		while let Some(message) = self.unpin_message_rx.next().await {
			match message {
				UnpinWorkerMessage::AnnouncePin(hash) => self.handle_announce_message(hash),
				UnpinWorkerMessage::Unpin(hash) =>
					if self.handle_unpin_message(hash).is_err() {
						return
					},
			}
		}
		log::debug!(target: LOG_TARGET, "Terminating unpin-worker, stream terminated.")
	}
}

#[cfg(test)]
mod tests {
}
