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

use crate::LOG_TARGET;
use log::trace;
use sc_network_common::sync::message;
use sc_network_types::PeerId;
use sp_arithmetic::traits::Saturating;
use sp_runtime::traits::{Block as BlockT, NumberFor, One};
use std::{
	cmp,
	collections::{BTreeMap, HashMap},
	ops::Range,
};

/// Block data with origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockData<B: BlockT> {
	/// The Block Message from the wire
	pub block: message::BlockData<B>,
	/// The peer, we received this from
	pub origin: Option<PeerId>,
}

#[derive(Debug)]
enum BlockRangeState<B: BlockT> {
	Downloading { len: NumberFor<B>, downloading: u32 },
	Complete(Vec<BlockData<B>>),
	Queued { len: NumberFor<B> },
}

impl<B: BlockT> BlockRangeState<B> {
	pub fn len(&self) -> NumberFor<B> {
		match *self {
			Self::Downloading { len, .. } => len,
			Self::Complete(ref blocks) => (blocks.len() as u32).into(),
			Self::Queued { len } => len,
		}
	}
}

/// A collection of blocks being downloaded.
#[derive(Default)]
pub struct BlockCollection<B: BlockT> {
	/// Downloaded blocks.
	blocks: BTreeMap<NumberFor<B>, BlockRangeState<B>>,
	peer_requests: HashMap<PeerId, NumberFor<B>>,
	/// Block ranges downloaded and queued for import.
	/// Maps start_hash => (start_num, end_num).
	queued_blocks: HashMap<B::Hash, (NumberFor<B>, NumberFor<B>)>,
}

impl<B: BlockT> BlockCollection<B> {
	/// Create a new instance.
	pub fn new() -> Self {
		Self {
			blocks: BTreeMap::new(),
			peer_requests: HashMap::new(),
			queued_blocks: HashMap::new(),
		}
	}

	/// Clear everything.
	pub fn clear(&mut self) {
		self.blocks.clear();
		self.peer_requests.clear();
	}

	/// Insert a set of blocks into collection.
	pub fn insert(&mut self, start: NumberFor<B>, blocks: Vec<message::BlockData<B>>, who: PeerId) {
		if blocks.is_empty() {
			return;
		}

		match self.blocks.get(&start) {
			Some(&BlockRangeState::Downloading { .. }) => {
				trace!(target: LOG_TARGET, "Inserting block data still marked as being downloaded: {}", start);
			},
			Some(BlockRangeState::Complete(existing)) if existing.len() >= blocks.len() => {
				trace!(target: LOG_TARGET, "Ignored block data already downloaded: {}", start);
				return;
			},
			_ => (),
		}

		self.blocks.insert(
			start,
			BlockRangeState::Complete(
				blocks.into_iter().map(|b| BlockData { origin: Some(who), block: b }).collect(),
			),
		);
	}

	/// Returns a set of block hashes that require a header download. The returned set is marked as
	/// being downloaded.
	pub fn needed_blocks(
		&mut self,
		who: PeerId,
		count: u32,
		peer_best: NumberFor<B>,
		common: NumberFor<B>,
		max_parallel: u32,
		max_ahead: u32,
	) -> Option<Range<NumberFor<B>>> {
		if peer_best <= common {
			// Bail out early
			return None;
		}
		// First block number that we need to download
		let first_different = common + <NumberFor<B>>::one();
		let count = (count as u32).into();
		let (mut range, downloading) = {
			// Iterate through the ranges in `self.blocks` looking for a range to download
			let mut downloading_iter = self.blocks.iter().peekable();
			let mut prev: Option<(&NumberFor<B>, &BlockRangeState<B>)> = None;
			loop {
				let next = downloading_iter.next();
				break match (prev, next) {
					// If we are already downloading this range, request it from `max_parallel`
					// peers (`max_parallel = 5` by default).
					// Do not request already downloading range from peers with common number above
					// the range start.
					(Some((start, &BlockRangeState::Downloading { ref len, downloading })), _)
						if downloading < max_parallel && *start >= first_different =>
					{
						(*start..*start + *len, downloading)
					},
					// If there is a gap between ranges requested, download this gap unless the peer
					// has common number above the gap start
					(Some((start, r)), Some((next_start, _)))
						if *start + r.len() < *next_start &&
							*start + r.len() >= first_different =>
					{
						(*start + r.len()..cmp::min(*next_start, *start + r.len() + count), 0)
					},
					// Download `count` blocks after the last range requested unless the peer
					// has common number above this new range
					(Some((start, r)), None) if *start + r.len() >= first_different => {
						(*start + r.len()..*start + r.len() + count, 0)
					},
					// If there are no ranges currently requested, download `count` blocks after
					// `common` number
					(None, None) => (first_different..first_different + count, 0),
					// If the first range starts above `common + 1`, download the gap at the start
					(None, Some((start, _))) if *start > first_different => {
						(first_different..cmp::min(first_different + count, *start), 0)
					},
					// Move on to the next range pair
					_ => {
						prev = next;
						continue;
					},
				};
			}
		};
		// crop to peers best
		if range.start > peer_best {
			trace!(target: LOG_TARGET, "Out of range for peer {} ({} vs {})", who, range.start, peer_best);
			return None;
		}

		range.end = cmp::min(peer_best.saturating_add(One::one()), range.end);

		if self
			.blocks
			.iter()
			.next()
			.map_or(false, |(n, _)| range.start > *n + max_ahead.into())
		{
			trace!(target: LOG_TARGET, "Too far ahead for peer {} ({})", who, range.start);
			return None;
		}

		if range.end <= range.start {
			debug_assert!(
				false,
				"Empty range {:?}, count={}, peer_best={}, common={}, blocks={:?}",
				range, count, peer_best, common, self.blocks
			);
			trace!(
				target: LOG_TARGET,
				"Empty range for peer {who}: {range:?}, count={count}, peer_best={peer_best}, common={common}",
			);
			return None;
		}

		self.peer_requests.insert(who, range.start);
		self.blocks.insert(
			range.start,
			BlockRangeState::Downloading {
				len: range.end - range.start,
				downloading: downloading + 1,
			},
		);

		Some(range)
	}

	/// Get a valid chain of blocks ordered in descending order and ready for importing into
	/// the blockchain.
	/// `from` is the maximum block number for the start of the range that we are interested in.
	/// The function will return empty Vec if the first block ready is higher than `from`.
	/// For each returned block hash `clear_queued` must be called at some later stage.
	pub fn ready_blocks(&mut self, from: NumberFor<B>) -> Vec<BlockData<B>> {
		let mut ready = Vec::new();

		let mut prev = from;
		for (&start, range_data) in &mut self.blocks {
			if start > prev {
				break;
			}
			let len = match range_data {
				BlockRangeState::Complete(blocks) => {
					let len = (blocks.len() as u32).into();
					prev = start + len;
					if let Some(BlockData { block, .. }) = blocks.first() {
						self.queued_blocks
							.insert(block.hash, (start, start + (blocks.len() as u32).into()));
					}
					// Remove all elements from `blocks` and add them to `ready`
					ready.append(blocks);
					len
				},
				BlockRangeState::Queued { .. } => continue,
				_ => break,
			};
			*range_data = BlockRangeState::Queued { len };
		}
		trace!(target: LOG_TARGET, "{} blocks ready for import", ready.len());
		ready
	}

	pub fn clear_queued(&mut self, hash: &B::Hash) {
		if let Some((from, to)) = self.queued_blocks.remove(hash) {
			let mut block_num = from;
			while block_num < to {
				self.blocks.remove(&block_num);
				block_num += One::one();
			}
			trace!(target: LOG_TARGET, "Cleared blocks from {:?} to {:?}", from, to);
		}
	}

	pub fn clear_peer_download(&mut self, who: &PeerId) {
		if let Some(start) = self.peer_requests.remove(who) {
			let remove = match self.blocks.get_mut(&start) {
				Some(&mut BlockRangeState::Downloading { ref mut downloading, .. })
					if *downloading > 1 =>
				{
					*downloading -= 1;
					false
				},
				Some(&mut BlockRangeState::Downloading { .. }) => true,
				_ => false,
			};
			if remove {
				self.blocks.remove(&start);
			}
		}
	}
}

#[cfg(test)]
mod test {
}
