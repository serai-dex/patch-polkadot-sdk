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

//! Canonicalization window.
//! Maintains trees of block overlays and allows discarding trees/roots
//! The overlays are added in `insert` and removed in `canonicalize`.

use crate::{LOG_TARGET, LOG_TARGET_PIN};

use super::{to_meta_key, ChangeSet, CommitSet, DBValue, Error, Hash, MetaDb, StateDbError};
use codec::{Decode, Encode};
use log::trace;
use std::collections::{hash_map::Entry, HashMap, VecDeque};

const NON_CANONICAL_JOURNAL: &[u8] = b"noncanonical_journal";
pub(crate) const LAST_CANONICAL: &[u8] = b"last_canonical";
const MAX_BLOCKS_PER_LEVEL: u64 = 32;

/// See module documentation.
pub struct NonCanonicalOverlay<BlockHash: Hash, Key: Hash> {
	last_canonicalized: Option<(BlockHash, u64)>,
	levels: VecDeque<OverlayLevel<BlockHash, Key>>,
	parents: HashMap<BlockHash, BlockHash>,
	values: HashMap<Key, (u32, DBValue)>, // ref counted
	// would be deleted but kept around because block is pinned, ref counted.
	pinned: HashMap<BlockHash, u32>,
	pinned_insertions: HashMap<BlockHash, (Vec<Key>, u32)>,
	pinned_canonincalized: Vec<BlockHash>,
}

#[cfg_attr(test, derive(PartialEq, Debug))]
struct OverlayLevel<BlockHash: Hash, Key: Hash> {
	blocks: Vec<BlockOverlay<BlockHash, Key>>,
	used_indices: u64, // Bitmask of available journal indices.
}

impl<BlockHash: Hash, Key: Hash> OverlayLevel<BlockHash, Key> {
	fn push(&mut self, overlay: BlockOverlay<BlockHash, Key>) {
		self.used_indices |= 1 << overlay.journal_index;
		self.blocks.push(overlay)
	}

	fn available_index(&self) -> u64 {
		self.used_indices.trailing_ones() as u64
	}

	fn remove(&mut self, index: usize) -> BlockOverlay<BlockHash, Key> {
		self.used_indices &= !(1 << self.blocks[index].journal_index);
		self.blocks.remove(index)
	}

	fn new() -> OverlayLevel<BlockHash, Key> {
		OverlayLevel { blocks: Vec::new(), used_indices: 0 }
	}
}

#[derive(Encode, Decode)]
struct JournalRecord<BlockHash: Hash, Key: Hash> {
	hash: BlockHash,
	parent_hash: BlockHash,
	inserted: Vec<(Key, DBValue)>,
	deleted: Vec<Key>,
}

fn to_journal_key(block: u64, index: u64) -> Vec<u8> {
	to_meta_key(NON_CANONICAL_JOURNAL, &(block, index))
}

#[cfg_attr(test, derive(PartialEq, Debug))]
struct BlockOverlay<BlockHash: Hash, Key: Hash> {
	hash: BlockHash,
	journal_index: u64,
	journal_key: Vec<u8>,
	inserted: Vec<Key>,
	deleted: Vec<Key>,
}

fn insert_values<Key: Hash>(
	values: &mut HashMap<Key, (u32, DBValue)>,
	inserted: Vec<(Key, DBValue)>,
) {
	for (k, v) in inserted {
		debug_assert!(values.get(&k).map_or(true, |(_, value)| *value == v));
		let (ref mut counter, _) = values.entry(k).or_insert_with(|| (0, v));
		*counter += 1;
	}
}

fn discard_values<Key: Hash>(values: &mut HashMap<Key, (u32, DBValue)>, inserted: Vec<Key>) {
	for k in inserted {
		match values.entry(k) {
			Entry::Occupied(mut e) => {
				let (ref mut counter, _) = e.get_mut();
				*counter -= 1;
				if *counter == 0 {
					e.remove_entry();
				}
			},
			Entry::Vacant(_) => {
				debug_assert!(false, "Trying to discard missing value");
			},
		}
	}
}

fn discard_descendants<BlockHash: Hash, Key: Hash>(
	levels: &mut (&mut [OverlayLevel<BlockHash, Key>], &mut [OverlayLevel<BlockHash, Key>]),
	values: &mut HashMap<Key, (u32, DBValue)>,
	parents: &mut HashMap<BlockHash, BlockHash>,
	pinned: &HashMap<BlockHash, u32>,
	pinned_insertions: &mut HashMap<BlockHash, (Vec<Key>, u32)>,
	hash: &BlockHash,
) -> u32 {
	let (first, mut remainder) = if let Some((first, rest)) = levels.0.split_first_mut() {
		(Some(first), (rest, &mut *levels.1))
	} else if let Some((first, rest)) = levels.1.split_first_mut() {
		(Some(first), (&mut *levels.0, rest))
	} else {
		(None, (&mut *levels.0, &mut *levels.1))
	};
	let mut pinned_children = 0;
	if let Some(level) = first {
		while let Some(i) = level.blocks.iter().position(|overlay| {
			parents
				.get(&overlay.hash)
				.expect("there is a parent entry for each entry in levels") ==
				hash
		}) {
			let overlay = level.remove(i);
			let mut num_pinned = discard_descendants(
				&mut remainder,
				values,
				parents,
				pinned,
				pinned_insertions,
				&overlay.hash,
			);
			if pinned.contains_key(&overlay.hash) {
				num_pinned += 1;
			}
			if num_pinned != 0 {
				// save to be discarded later.
				pinned_insertions.insert(overlay.hash.clone(), (overlay.inserted, num_pinned));
				pinned_children += num_pinned;
			} else {
				// discard immediately.
				parents.remove(&overlay.hash);
				discard_values(values, overlay.inserted);
			}
		}
	}
	pinned_children
}

impl<BlockHash: Hash, Key: Hash> NonCanonicalOverlay<BlockHash, Key> {
	/// Creates a new instance. Does not expect any metadata to be present in the DB.
	pub fn new<D: MetaDb>(db: &D) -> Result<NonCanonicalOverlay<BlockHash, Key>, Error<D::Error>> {
		let last_canonicalized =
			db.get_meta(&to_meta_key(LAST_CANONICAL, &())).map_err(Error::Db)?;
		let last_canonicalized = last_canonicalized
			.map(|buffer| <(BlockHash, u64)>::decode(&mut buffer.as_slice()))
			.transpose()?;
		let mut levels = VecDeque::new();
		let mut parents = HashMap::new();
		let mut values = HashMap::new();
		if let Some((ref hash, mut block)) = last_canonicalized {
			// read the journal
			trace!(
				target: LOG_TARGET,
				"Reading uncanonicalized journal. Last canonicalized #{} ({:?})",
				block,
				hash
			);
			let mut total: u64 = 0;
			block += 1;
			loop {
				let mut level = OverlayLevel::new();
				for index in 0..MAX_BLOCKS_PER_LEVEL {
					let journal_key = to_journal_key(block, index);
					if let Some(record) = db.get_meta(&journal_key).map_err(Error::Db)? {
						let record: JournalRecord<BlockHash, Key> =
							Decode::decode(&mut record.as_slice())?;
						let inserted = record.inserted.iter().map(|(k, _)| k.clone()).collect();
						let overlay = BlockOverlay {
							hash: record.hash.clone(),
							journal_index: index,
							journal_key,
							inserted,
							deleted: record.deleted,
						};
						insert_values(&mut values, record.inserted);
						trace!(
							target: LOG_TARGET,
							"Uncanonicalized journal entry {}.{} ({:?}) ({} inserted, {} deleted)",
							block,
							index,
							record.hash,
							overlay.inserted.len(),
							overlay.deleted.len()
						);
						level.push(overlay);
						parents.insert(record.hash, record.parent_hash);
						total += 1;
					}
				}
				if level.blocks.is_empty() {
					break;
				}
				levels.push_back(level);
				block += 1;
			}
			trace!(
				target: LOG_TARGET,
				"Finished reading uncanonicalized journal, {} entries",
				total
			);
		}
		Ok(NonCanonicalOverlay {
			last_canonicalized,
			levels,
			parents,
			pinned: Default::default(),
			pinned_insertions: Default::default(),
			values,
			pinned_canonincalized: Default::default(),
		})
	}

	/// Insert a new block into the overlay. If inserted on the second level or lover expects parent
	/// to be present in the window.
	pub fn insert(
		&mut self,
		hash: &BlockHash,
		number: u64,
		parent_hash: &BlockHash,
		changeset: ChangeSet<Key>,
	) -> Result<CommitSet<Key>, StateDbError> {
		let mut commit = CommitSet::default();
		let front_block_number = self.front_block_number();
		if self.levels.is_empty() && self.last_canonicalized.is_none() && number > 0 {
			// assume that parent was canonicalized
			let last_canonicalized = (parent_hash.clone(), number - 1);
			commit
				.meta
				.inserted
				.push((to_meta_key(LAST_CANONICAL, &()), last_canonicalized.encode()));
			self.last_canonicalized = Some(last_canonicalized);
		} else if self.last_canonicalized.is_some() {
			if number < front_block_number || number > front_block_number + self.levels.len() as u64
			{
				trace!(
					target: LOG_TARGET,
					"Failed to insert block {}, current is {} .. {})",
					number,
					front_block_number,
					front_block_number + self.levels.len() as u64,
				);
				return Err(StateDbError::InvalidBlockNumber);
			}
			// check for valid parent if inserting on second level or higher
			if number == front_block_number {
				if !self
					.last_canonicalized
					.as_ref()
					.map_or(false, |&(ref h, n)| h == parent_hash && n == number - 1)
				{
					return Err(StateDbError::InvalidParent);
				}
			} else if !self.parents.contains_key(parent_hash) {
				return Err(StateDbError::InvalidParent);
			}
		}
		let level = if self.levels.is_empty() ||
			number == front_block_number + self.levels.len() as u64
		{
			self.levels.push_back(OverlayLevel::new());
			self.levels.back_mut().expect("can't be empty after insertion")
		} else {
			self.levels.get_mut((number - front_block_number) as usize)
				.expect("number is [front_block_number .. front_block_number + levels.len()) is asserted in precondition")
		};

		if level.blocks.len() >= MAX_BLOCKS_PER_LEVEL as usize {
			trace!(
				target: LOG_TARGET,
				"Too many sibling blocks at #{number}: {:?}",
				level.blocks.iter().map(|b| &b.hash).collect::<Vec<_>>()
			);
			return Err(StateDbError::TooManySiblingBlocks { number });
		}
		if level.blocks.iter().any(|b| b.hash == *hash) {
			return Err(StateDbError::BlockAlreadyExists);
		}

		let index = level.available_index();
		let journal_key = to_journal_key(number, index);

		let inserted = changeset.inserted.iter().map(|(k, _)| k.clone()).collect();
		let overlay = BlockOverlay {
			hash: hash.clone(),
			journal_index: index,
			journal_key: journal_key.clone(),
			inserted,
			deleted: changeset.deleted.clone(),
		};
		level.push(overlay);
		self.parents.insert(hash.clone(), parent_hash.clone());
		let journal_record = JournalRecord {
			hash: hash.clone(),
			parent_hash: parent_hash.clone(),
			inserted: changeset.inserted,
			deleted: changeset.deleted,
		};
		commit.meta.inserted.push((journal_key, journal_record.encode()));
		trace!(
			target: LOG_TARGET,
			"Inserted uncanonicalized changeset {}.{} {:?} ({} inserted, {} deleted)",
			number,
			index,
			hash,
			journal_record.inserted.len(),
			journal_record.deleted.len()
		);
		insert_values(&mut self.values, journal_record.inserted);
		Ok(commit)
	}

	fn discard_journals(
		&self,
		level_index: usize,
		discarded_journals: &mut Vec<Vec<u8>>,
		hash: &BlockHash,
	) {
		if let Some(level) = self.levels.get(level_index) {
			level.blocks.iter().for_each(|overlay| {
				let parent = self
					.parents
					.get(&overlay.hash)
					.expect("there is a parent entry for each entry in levels")
					.clone();
				if parent == *hash {
					discarded_journals.push(overlay.journal_key.clone());
					self.discard_journals(level_index + 1, discarded_journals, &overlay.hash);
				}
			});
		}
	}

	fn front_block_number(&self) -> u64 {
		self.last_canonicalized.as_ref().map(|&(_, n)| n + 1).unwrap_or(0)
	}

	pub fn last_canonicalized_block_number(&self) -> Option<u64> {
		self.last_canonicalized.as_ref().map(|&(_, n)| n)
	}

	/// Confirm that all changes made to commit sets are on disk. Allows for temporarily pinned
	/// blocks to be released.
	pub fn sync(&mut self) {
		let mut pinned = std::mem::take(&mut self.pinned_canonincalized);
		for hash in pinned.iter() {
			self.unpin(hash)
		}
		pinned.clear();
		// Reuse the same memory buffer
		self.pinned_canonincalized = pinned;
	}

	/// Select a top-level root and canonicalized it. Discards all sibling subtrees and the root.
	/// Add a set of changes of the canonicalized block to `CommitSet`
	/// Return the block number of the canonicalized block
	pub fn canonicalize(
		&mut self,
		hash: &BlockHash,
		commit: &mut CommitSet<Key>,
	) -> Result<u64, StateDbError> {
		trace!(target: LOG_TARGET, "Canonicalizing {:?}", hash);
		let level = match self.levels.pop_front() {
			Some(level) => level,
			None => return Err(StateDbError::InvalidBlock),
		};
		let index = level
			.blocks
			.iter()
			.position(|overlay| overlay.hash == *hash)
			.ok_or(StateDbError::InvalidBlock)?;

		// No failures are possible beyond this point.

		// Force pin canonicalized block so that it is no discarded immediately
		self.pin(hash);
		self.pinned_canonincalized.push(hash.clone());

		let mut discarded_journals = Vec::new();
		for (i, overlay) in level.blocks.into_iter().enumerate() {
			let mut pinned_children = 0;
			// That's the one we need to canonicalize
			if i == index {
				commit.data.inserted.extend(overlay.inserted.iter().map(|k| {
					(
						k.clone(),
						self.values
							.get(k)
							.expect("For each key in overlays there's a value in values")
							.1
							.clone(),
					)
				}));
				commit.data.deleted.extend(overlay.deleted.clone());
			} else {
				// Discard this overlay
				self.discard_journals(0, &mut discarded_journals, &overlay.hash);
				pinned_children = discard_descendants(
					&mut self.levels.as_mut_slices(),
					&mut self.values,
					&mut self.parents,
					&self.pinned,
					&mut self.pinned_insertions,
					&overlay.hash,
				);
			}
			if self.pinned.contains_key(&overlay.hash) {
				pinned_children += 1;
			}
			if pinned_children != 0 {
				self.pinned_insertions
					.insert(overlay.hash.clone(), (overlay.inserted, pinned_children));
			} else {
				self.parents.remove(&overlay.hash);
				discard_values(&mut self.values, overlay.inserted);
			}
			discarded_journals.push(overlay.journal_key.clone());
		}
		commit.meta.deleted.append(&mut discarded_journals);

		let canonicalized = (hash.clone(), self.front_block_number());
		commit
			.meta
			.inserted
			.push((to_meta_key(LAST_CANONICAL, &()), canonicalized.encode()));
		trace!(target: LOG_TARGET, "Discarding {} records", commit.meta.deleted.len());

		let num = canonicalized.1;
		self.last_canonicalized = Some(canonicalized);
		Ok(num)
	}

	/// Get a value from the node overlay. This searches in every existing changeset.
	pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<DBValue>
	where
		Key: std::borrow::Borrow<Q>,
		Q: std::hash::Hash + Eq,
	{
		self.values.get(key).map(|v| v.1.clone())
	}

	/// Check if the block is in the canonicalization queue.
	pub fn have_block(&self, hash: &BlockHash) -> bool {
		self.parents.contains_key(hash)
	}

	/// Revert a single level. Returns commit set that deletes the journal or `None` if not
	/// possible.
	pub fn revert_one(&mut self) -> Option<CommitSet<Key>> {
		self.levels.pop_back().map(|level| {
			let mut commit = CommitSet::default();
			for overlay in level.blocks.into_iter() {
				commit.meta.deleted.push(overlay.journal_key);
				self.parents.remove(&overlay.hash);
				discard_values(&mut self.values, overlay.inserted);
			}
			commit
		})
	}

	/// Revert a single block. Returns commit set that deletes the journal or `None` if not
	/// possible.
	pub fn remove(&mut self, hash: &BlockHash) -> Option<CommitSet<Key>> {
		let mut commit = CommitSet::default();
		let level_count = self.levels.len();
		for (level_index, level) in self.levels.iter_mut().enumerate().rev() {
			let index = match level.blocks.iter().position(|overlay| &overlay.hash == hash) {
				Some(index) => index,
				None => continue,
			};
			// Check that it does not have any children
			if (level_index != level_count - 1) && self.parents.values().any(|h| h == hash) {
				log::debug!(target: LOG_TARGET, "Trying to remove block {:?} with children", hash);
				return None;
			}
			let overlay = level.remove(index);
			commit.meta.deleted.push(overlay.journal_key);
			self.parents.remove(&overlay.hash);
			discard_values(&mut self.values, overlay.inserted);
			break;
		}
		if self.levels.back().map_or(false, |l| l.blocks.is_empty()) {
			self.levels.pop_back();
		}
		if !commit.meta.deleted.is_empty() {
			Some(commit)
		} else {
			None
		}
	}

	/// Pin state values in memory
	pub fn pin(&mut self, hash: &BlockHash) {
		let refs = self.pinned.entry(hash.clone()).or_default();
		if *refs == 0 {
			trace!(target: LOG_TARGET_PIN, "Pinned non-canon block: {:?}", hash);
		}
		*refs += 1;
	}

	/// Discard pinned state
	pub fn unpin(&mut self, hash: &BlockHash) {
		let removed = match self.pinned.entry(hash.clone()) {
			Entry::Occupied(mut entry) => {
				*entry.get_mut() -= 1;
				if *entry.get() == 0 {
					entry.remove();
					true
				} else {
					false
				}
			},
			Entry::Vacant(_) => false,
		};

		if removed {
			let mut parent = Some(hash.clone());
			while let Some(hash) = parent {
				parent = self.parents.get(&hash).cloned();
				match self.pinned_insertions.entry(hash.clone()) {
					Entry::Occupied(mut entry) => {
						entry.get_mut().1 -= 1;
						if entry.get().1 == 0 {
							let (inserted, _) = entry.remove();
							trace!(
								target: LOG_TARGET_PIN,
								"Discarding unpinned non-canon block: {:?}",
								hash
							);
							discard_values(&mut self.values, inserted);
							self.parents.remove(&hash);
						}
					},
					Entry::Vacant(_) => break,
				};
			}
		}
	}
}

#[cfg(test)]
mod tests {
}
