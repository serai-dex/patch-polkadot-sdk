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

//! Pruning window.
//!
//! For each block we maintain a list of nodes pending deletion.
//! There is also a global index of node key to block number.
//! If a node is re-inserted into the window it gets removed from
//! the death list.
//! The changes are journaled in the DB.

use crate::{
	noncanonical::LAST_CANONICAL, to_meta_key, CommitSet, Error, Hash, MetaDb, StateDbError,
	DEFAULT_MAX_BLOCK_CONSTRAINT, LOG_TARGET,
};
use codec::{Decode, Encode};
use log::trace;
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) const LAST_PRUNED: &[u8] = b"last_pruned";
const PRUNING_JOURNAL: &[u8] = b"pruning_journal";

/// See module documentation.
pub struct RefWindow<BlockHash: Hash, Key: Hash, D: MetaDb> {
	/// A queue of blocks keep tracking keys that should be deleted for each block in the
	/// pruning window.
	queue: DeathRowQueue<BlockHash, Key, D>,
	/// Block number that is next to be pruned.
	base: u64,
}

/// `DeathRowQueue` used to keep track of blocks in the pruning window, there are two flavors:
/// - `Mem`, used when the backend database do not supports reference counting, keep all
/// 	blocks in memory, and keep track of re-inserted keys to not delete them when pruning
/// - `DbBacked`, used when the backend database supports reference counting, only keep
/// 	a few number of blocks in memory and load more blocks on demand
enum DeathRowQueue<BlockHash: Hash, Key: Hash, D: MetaDb> {
	Mem {
		/// A queue of keys that should be deleted for each block in the pruning window.
		death_rows: VecDeque<DeathRow<BlockHash, Key>>,
		/// An index that maps each key from `death_rows` to block number.
		death_index: HashMap<Key, u64>,
	},
	DbBacked {
		// The backend database
		db: D,
		/// A queue of keys that should be deleted for each block in the pruning window.
		/// Only caching the first few blocks of the pruning window, blocks inside are
		/// successive and ordered by block number
		cache: VecDeque<DeathRow<BlockHash, Key>>,
		/// A soft limit of the cache's size
		cache_capacity: usize,
		/// Last block number added to the window
		last: Option<u64>,
	},
}

impl<BlockHash: Hash, Key: Hash, D: MetaDb> DeathRowQueue<BlockHash, Key, D> {
	/// Return a `DeathRowQueue` that all blocks are keep in memory
	fn new_mem(db: &D, base: u64) -> Result<DeathRowQueue<BlockHash, Key, D>, Error<D::Error>> {
		let mut block = base;
		let mut queue = DeathRowQueue::<BlockHash, Key, D>::Mem {
			death_rows: VecDeque::new(),
			death_index: HashMap::new(),
		};
		// read the journal
		trace!(
			target: LOG_TARGET,
			"Reading pruning journal for the memory queue. Pending #{}",
			base,
		);
		loop {
			let journal_key = to_journal_key(block);
			match db.get_meta(&journal_key).map_err(Error::Db)? {
				Some(record) => {
					let record: JournalRecord<BlockHash, Key> =
						Decode::decode(&mut record.as_slice())?;
					trace!(
						target: LOG_TARGET,
						"Pruning journal entry {} ({} inserted, {} deleted)",
						block,
						record.inserted.len(),
						record.deleted.len(),
					);
					queue.import(base, block, record);
				},
				None => break,
			}
			block += 1;
		}
		Ok(queue)
	}

	/// Return a `DeathRowQueue` that backed by an database, and only keep a few number
	/// of blocks in memory
	fn new_db_backed(
		db: D,
		base: u64,
		last: Option<u64>,
		window_size: u32,
	) -> Result<DeathRowQueue<BlockHash, Key, D>, Error<D::Error>> {
		// limit the cache capacity from 1 to `DEFAULT_MAX_BLOCK_CONSTRAINT`
		let cache_capacity = window_size.clamp(1, DEFAULT_MAX_BLOCK_CONSTRAINT) as usize;
		let mut cache = VecDeque::with_capacity(cache_capacity);
		trace!(
			target: LOG_TARGET,
			"Reading pruning journal for the database-backed queue. Pending #{}",
			base
		);
		DeathRowQueue::load_batch_from_db(&db, &mut cache, base, cache_capacity)?;
		Ok(DeathRowQueue::DbBacked { db, cache, cache_capacity, last })
	}

	/// import a new block to the back of the queue
	fn import(&mut self, base: u64, num: u64, journal_record: JournalRecord<BlockHash, Key>) {
		let JournalRecord { hash, inserted, deleted } = journal_record;
		trace!(target: LOG_TARGET, "Importing {}, base={}", num, base);
		match self {
			DeathRowQueue::DbBacked { cache, cache_capacity, last, .. } => {
				// If the new block continues cached range and there is space, load it directly into
				// cache.
				if num == base + cache.len() as u64 && cache.len() < *cache_capacity {
					trace!(target: LOG_TARGET, "Adding to DB backed cache {:?} (#{})", hash, num);
					cache.push_back(DeathRow { hash, deleted: deleted.into_iter().collect() });
				}
				*last = Some(num);
			},
			DeathRowQueue::Mem { death_rows, death_index } => {
				// remove all re-inserted keys from death rows
				for k in inserted {
					if let Some(block) = death_index.remove(&k) {
						death_rows[(block - base) as usize].deleted.remove(&k);
					}
				}
				// add new keys
				let imported_block = base + death_rows.len() as u64;
				for k in deleted.iter() {
					death_index.insert(k.clone(), imported_block);
				}
				death_rows.push_back(DeathRow { hash, deleted: deleted.into_iter().collect() });
			},
		}
	}

	/// Pop out one block from the front of the queue, `base` is the block number
	/// of the first block of the queue
	fn pop_front(
		&mut self,
		base: u64,
	) -> Result<Option<DeathRow<BlockHash, Key>>, Error<D::Error>> {
		match self {
			DeathRowQueue::DbBacked { db, cache, cache_capacity, .. } => {
				if cache.is_empty() {
					DeathRowQueue::load_batch_from_db(db, cache, base, *cache_capacity)?;
				}
				Ok(cache.pop_front())
			},
			DeathRowQueue::Mem { death_rows, death_index } => match death_rows.pop_front() {
				Some(row) => {
					for k in row.deleted.iter() {
						death_index.remove(k);
					}
					Ok(Some(row))
				},
				None => Ok(None),
			},
		}
	}

	/// Load a batch of blocks from the backend database into `cache`, starting from `base` and up
	/// to `base + cache_capacity`
	fn load_batch_from_db(
		db: &D,
		cache: &mut VecDeque<DeathRow<BlockHash, Key>>,
		base: u64,
		cache_capacity: usize,
	) -> Result<(), Error<D::Error>> {
		let start = base + cache.len() as u64;
		let batch_size = cache_capacity;
		for i in 0..batch_size as u64 {
			match load_death_row_from_db::<BlockHash, Key, D>(db, start + i)? {
				Some(row) => {
					cache.push_back(row);
				},
				None => break,
			}
		}
		Ok(())
	}

	/// Check if the block at the given `index` of the queue exist
	/// it is the caller's responsibility to ensure `index` won't be out of bounds
	fn have_block(&self, hash: &BlockHash, index: usize) -> HaveBlock {
		match self {
			DeathRowQueue::DbBacked { cache, .. } => {
				if cache.len() > index {
					(cache[index].hash == *hash).into()
				} else {
					// The block is not in the cache but it still may exist on disk.
					HaveBlock::Maybe
				}
			},
			DeathRowQueue::Mem { death_rows, .. } => (death_rows[index].hash == *hash).into(),
		}
	}

	/// Return the number of block in the pruning window
	fn len(&self, base: u64) -> u64 {
		match self {
			DeathRowQueue::DbBacked { last, .. } => last.map_or(0, |l| l + 1 - base),
			DeathRowQueue::Mem { death_rows, .. } => death_rows.len() as u64,
		}
	}

	#[cfg(test)]
	fn get_mem_queue_state(
		&self,
	) -> Option<(&VecDeque<DeathRow<BlockHash, Key>>, &HashMap<Key, u64>)> {
		match self {
			DeathRowQueue::DbBacked { .. } => None,
			DeathRowQueue::Mem { death_rows, death_index } => Some((death_rows, death_index)),
		}
	}

	#[cfg(test)]
	fn get_db_backed_queue_state(
		&self,
	) -> Option<(&VecDeque<DeathRow<BlockHash, Key>>, Option<u64>)> {
		match self {
			DeathRowQueue::DbBacked { cache, last, .. } => Some((cache, *last)),
			DeathRowQueue::Mem { .. } => None,
		}
	}
}

fn load_death_row_from_db<BlockHash: Hash, Key: Hash, D: MetaDb>(
	db: &D,
	block: u64,
) -> Result<Option<DeathRow<BlockHash, Key>>, Error<D::Error>> {
	let journal_key = to_journal_key(block);
	match db.get_meta(&journal_key).map_err(Error::Db)? {
		Some(record) => {
			let JournalRecord { hash, deleted, .. } = Decode::decode(&mut record.as_slice())?;
			Ok(Some(DeathRow { hash, deleted: deleted.into_iter().collect() }))
		},
		None => Ok(None),
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeathRow<BlockHash: Hash, Key: Hash> {
	hash: BlockHash,
	deleted: HashSet<Key>,
}

#[derive(Encode, Decode, Default)]
struct JournalRecord<BlockHash: Hash, Key: Hash> {
	hash: BlockHash,
	inserted: Vec<Key>,
	deleted: Vec<Key>,
}

fn to_journal_key(block: u64) -> Vec<u8> {
	to_meta_key(PRUNING_JOURNAL, &block)
}

/// The result return by `RefWindow::have_block`
#[derive(Debug, PartialEq, Eq)]
pub enum HaveBlock {
	/// Definitely don't have this block.
	No,
	/// May or may not have this block, need further checking
	Maybe,
	/// Definitely has this block
	Yes,
}

impl From<bool> for HaveBlock {
	fn from(have: bool) -> Self {
		if have {
			HaveBlock::Yes
		} else {
			HaveBlock::No
		}
	}
}

impl<BlockHash: Hash, Key: Hash, D: MetaDb> RefWindow<BlockHash, Key, D> {
	pub fn new(
		db: D,
		window_size: u32,
		count_insertions: bool,
	) -> Result<RefWindow<BlockHash, Key, D>, Error<D::Error>> {
		// the block number of the first block in the queue or the next block number if the queue is
		// empty
		let base = match db.get_meta(&to_meta_key(LAST_PRUNED, &())).map_err(Error::Db)? {
			Some(buffer) => u64::decode(&mut buffer.as_slice())? + 1,
			None => 0,
		};
		// the block number of the last block in the queue
		let last_canonicalized_number =
			match db.get_meta(&to_meta_key(LAST_CANONICAL, &())).map_err(Error::Db)? {
				Some(buffer) => Some(<(BlockHash, u64)>::decode(&mut buffer.as_slice())?.1),
				None => None,
			};

		let queue = if count_insertions {
			// Highly scientific crafted number for deciding when to print the warning!
			//
			// Rocksdb doesn't support refcounting and requires that we load the entire pruning
			// window into the memory.
			if window_size > 1000 {
				log::warn!(
					target: LOG_TARGET,
					"Large pruning window of {window_size} detected! THIS CAN LEAD TO HIGH MEMORY USAGE AND CRASHES. \
					Reduce the pruning window or switch your database to paritydb."
				);
			}

			DeathRowQueue::new_mem(&db, base)?
		} else {
			let last = match last_canonicalized_number {
				Some(last_canonicalized_number) => {
					debug_assert!(last_canonicalized_number + 1 >= base);
					Some(last_canonicalized_number)
				},
				// None means `LAST_CANONICAL` is never been wrote, since the pruning journals are
				// in the same `CommitSet` as `LAST_CANONICAL`, it means no pruning journal have
				// ever been committed to the db, thus set `unload` to zero
				None => None,
			};
			DeathRowQueue::new_db_backed(db, base, last, window_size)?
		};

		Ok(RefWindow { queue, base })
	}

	pub fn window_size(&self) -> u64 {
		self.queue.len(self.base) as u64
	}

	/// Get the hash of the next pruning block
	pub fn next_hash(&mut self) -> Result<Option<BlockHash>, Error<D::Error>> {
		let res = match &mut self.queue {
			DeathRowQueue::DbBacked { db, cache, cache_capacity, .. } => {
				if cache.is_empty() {
					DeathRowQueue::load_batch_from_db(db, cache, self.base, *cache_capacity)?;
				}
				cache.front().map(|r| r.hash.clone())
			},
			DeathRowQueue::Mem { death_rows, .. } => death_rows.front().map(|r| r.hash.clone()),
		};
		Ok(res)
	}

	fn is_empty(&self) -> bool {
		self.window_size() == 0
	}

	// Check if a block is in the pruning window and not be pruned yet
	pub fn have_block(&self, hash: &BlockHash, number: u64) -> HaveBlock {
		// if the queue is empty or the block number exceed the pruning window, we definitely
		// do not have this block
		if self.is_empty() || number < self.base || number >= self.base + self.window_size() {
			return HaveBlock::No;
		}
		self.queue.have_block(hash, (number - self.base) as usize)
	}

	/// Prune next block. Expects at least one block in the window. Adds changes to `commit`.
	pub fn prune_one(&mut self, commit: &mut CommitSet<Key>) -> Result<(), Error<D::Error>> {
		if let Some(pruned) = self.queue.pop_front(self.base)? {
			trace!(target: LOG_TARGET, "Pruning {:?} ({} deleted)", pruned.hash, pruned.deleted.len());
			let index = self.base;
			commit.data.deleted.extend(pruned.deleted.into_iter());
			commit.meta.inserted.push((to_meta_key(LAST_PRUNED, &()), index.encode()));
			commit.meta.deleted.push(to_journal_key(self.base));
			self.base += 1;
			Ok(())
		} else {
			trace!(target: LOG_TARGET, "Trying to prune when there's nothing to prune");
			Err(Error::StateDb(StateDbError::BlockUnavailable))
		}
	}

	/// Add a change set to the window. Creates a journal record and pushes it to `commit`
	pub fn note_canonical(
		&mut self,
		hash: &BlockHash,
		number: u64,
		commit: &mut CommitSet<Key>,
	) -> Result<(), Error<D::Error>> {
		if self.base == 0 && self.is_empty() && number > 0 {
			// This branch is taken if the node imports the target block of a warp sync.
			// assume that the block was canonicalized
			self.base = number;
			// The parent of the block was the last block that got pruned.
			commit
				.meta
				.inserted
				.push((to_meta_key(LAST_PRUNED, &()), (number - 1).encode()));
		} else if (self.base + self.window_size()) != number {
			return Err(Error::StateDb(StateDbError::InvalidBlockNumber));
		}
		trace!(
			target: LOG_TARGET,
			"Adding to pruning window: {:?} ({} inserted, {} deleted)",
			hash,
			commit.data.inserted.len(),
			commit.data.deleted.len(),
		);
		let inserted = if matches!(self.queue, DeathRowQueue::Mem { .. }) {
			commit.data.inserted.iter().map(|(k, _)| k.clone()).collect()
		} else {
			Default::default()
		};
		let deleted = std::mem::take(&mut commit.data.deleted);
		let journal_record = JournalRecord { hash: hash.clone(), inserted, deleted };
		commit.meta.inserted.push((to_journal_key(number), journal_record.encode()));
		self.queue.import(self.base, number, journal_record);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
}
