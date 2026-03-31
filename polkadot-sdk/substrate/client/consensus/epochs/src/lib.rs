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

//! Generic utilities for epoch-based consensus engines.

pub mod migration;

use codec::{Decode, Encode};
use fork_tree::{FilterAction, ForkTree};
use sc_client_api::utils::is_descendent_of;
use sp_blockchain::{Error as ClientError, HeaderBackend, HeaderMetadata};
use sp_runtime::traits::{Block as BlockT, NumberFor, One, Zero};
use std::{
	borrow::{Borrow, BorrowMut},
	collections::BTreeMap,
	ops::{Add, Sub},
};

/// A builder for `is_descendent_of` functions.
pub trait IsDescendentOfBuilder<Hash> {
	/// The error returned by the function.
	type Error: std::error::Error;
	/// A function that can tell you if the second parameter is a descendent of
	/// the first.
	type IsDescendentOf: Fn(&Hash, &Hash) -> Result<bool, Self::Error>;

	/// Build an `is_descendent_of` function.
	///
	/// The `current` parameter can be `Some` with the details a fresh block whose
	/// details aren't yet stored, but its parent is.
	///
	/// The format of `current` when `Some` is `(current, current_parent)`.
	fn build_is_descendent_of(&self, current: Option<(Hash, Hash)>) -> Self::IsDescendentOf;
}

/// Produce a descendent query object given the client.
pub fn descendent_query<H, Block>(client: &H) -> HeaderBackendDescendentBuilder<&H, Block> {
	HeaderBackendDescendentBuilder(client, std::marker::PhantomData)
}

/// Wrapper to get around unconstrained type errors when implementing
/// `IsDescendentOfBuilder` for header backends.
pub struct HeaderBackendDescendentBuilder<H, Block>(H, std::marker::PhantomData<Block>);

impl<'a, H, Block> IsDescendentOfBuilder<Block::Hash>
	for HeaderBackendDescendentBuilder<&'a H, Block>
where
	H: HeaderBackend<Block> + HeaderMetadata<Block, Error = ClientError>,
	Block: BlockT,
{
	type Error = ClientError;
	type IsDescendentOf = Box<dyn Fn(&Block::Hash, &Block::Hash) -> Result<bool, ClientError> + 'a>;

	fn build_is_descendent_of(
		&self,
		current: Option<(Block::Hash, Block::Hash)>,
	) -> Self::IsDescendentOf {
		Box::new(is_descendent_of(self.0, current))
	}
}

/// Epoch data, distinguish whether it is genesis or not.
///
/// Once an epoch is created, it must have a known `start_slot` and `end_slot`, which cannot be
/// changed. Consensus engine may modify any other data in the epoch, if needed.
pub trait Epoch: std::fmt::Debug {
	/// Descriptor for the next epoch.
	type NextEpochDescriptor;
	/// Type of the slot number.
	type Slot: Ord + Copy + std::fmt::Debug;

	/// The starting slot of the epoch.
	fn start_slot(&self) -> Self::Slot;
	/// Produce the "end slot" of the epoch. This is NOT inclusive to the epoch,
	/// i.e. the slots covered by the epoch are `self.start_slot() .. self.end_slot()`.
	fn end_slot(&self) -> Self::Slot;
	/// Increment the epoch data, using the next epoch descriptor.
	fn increment(&self, descriptor: Self::NextEpochDescriptor) -> Self;
}

impl<'a, E: Epoch> From<&'a E> for EpochHeader<E> {
	fn from(epoch: &'a E) -> EpochHeader<E> {
		Self { start_slot: epoch.start_slot(), end_slot: epoch.end_slot() }
	}
}

/// Header of epoch data, consisting of start and end slot.
#[derive(Eq, PartialEq, Encode, Decode, Debug)]
pub struct EpochHeader<E: Epoch> {
	/// The starting slot of the epoch.
	pub start_slot: E::Slot,
	/// The end slot of the epoch. This is NOT inclusive to the epoch,
	/// i.e. the slots covered by the epoch are `self.start_slot() .. self.end_slot()`.
	pub end_slot: E::Slot,
}

impl<E: Epoch> Clone for EpochHeader<E> {
	fn clone(&self) -> Self {
		Self { start_slot: self.start_slot, end_slot: self.end_slot }
	}
}

/// Position of the epoch identifier.
#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub enum EpochIdentifierPosition {
	/// The identifier points to a genesis epoch `epoch_0`.
	Genesis0,
	/// The identifier points to a genesis epoch `epoch_1`.
	Genesis1,
	/// The identifier points to a regular epoch.
	Regular,
}

/// Epoch identifier.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub struct EpochIdentifier<Hash, Number> {
	/// Location of the epoch.
	pub position: EpochIdentifierPosition,
	/// Hash of the block when the epoch is signaled.
	pub hash: Hash,
	/// Number of the block when the epoch is signaled.
	pub number: Number,
}

/// The viable epoch under which a block can be verified.
///
/// If this is the first non-genesis block in the chain, then it will
/// hold an `UnimportedGenesis` epoch.
pub enum ViableEpoch<E, ERef = E> {
	/// Unimported genesis viable epoch data.
	UnimportedGenesis(E),
	/// Regular viable epoch data.
	Signaled(ERef),
}

impl<E, ERef> AsRef<E> for ViableEpoch<E, ERef>
where
	ERef: Borrow<E>,
{
	fn as_ref(&self) -> &E {
		match *self {
			ViableEpoch::UnimportedGenesis(ref e) => e,
			ViableEpoch::Signaled(ref e) => e.borrow(),
		}
	}
}

impl<E, ERef> AsMut<E> for ViableEpoch<E, ERef>
where
	ERef: BorrowMut<E>,
{
	fn as_mut(&mut self) -> &mut E {
		match *self {
			ViableEpoch::UnimportedGenesis(ref mut e) => e,
			ViableEpoch::Signaled(ref mut e) => e.borrow_mut(),
		}
	}
}

impl<E, ERef> ViableEpoch<E, ERef>
where
	E: Epoch + Clone,
	ERef: Borrow<E>,
{
	/// Extract the underlying epoch, disregarding the fact that a genesis
	/// epoch may be unimported.
	pub fn into_cloned_inner(self) -> E {
		match self {
			ViableEpoch::UnimportedGenesis(e) => e,
			ViableEpoch::Signaled(e) => e.borrow().clone(),
		}
	}

	/// Get cloned value for the viable epoch.
	pub fn into_cloned(self) -> ViableEpoch<E, E> {
		match self {
			ViableEpoch::UnimportedGenesis(e) => ViableEpoch::UnimportedGenesis(e),
			ViableEpoch::Signaled(e) => ViableEpoch::Signaled(e.borrow().clone()),
		}
	}

	/// Increment the epoch, yielding an `IncrementedEpoch` to be imported
	/// into the fork-tree.
	pub fn increment(&self, next_descriptor: E::NextEpochDescriptor) -> IncrementedEpoch<E> {
		let next = self.as_ref().increment(next_descriptor);
		let to_persist = match *self {
			ViableEpoch::UnimportedGenesis(ref epoch_0) => {
				PersistedEpoch::Genesis(epoch_0.clone(), next)
			},
			ViableEpoch::Signaled(_) => PersistedEpoch::Regular(next),
		};

		IncrementedEpoch(to_persist)
	}
}

/// Descriptor for a viable epoch.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ViableEpochDescriptor<Hash, Number, E: Epoch> {
	/// The epoch is an unimported genesis, with given start slot number.
	UnimportedGenesis(E::Slot),
	/// The epoch is signaled and has been imported, with given identifier and header.
	Signaled(EpochIdentifier<Hash, Number>, EpochHeader<E>),
}

impl<Hash, Number, E: Epoch> ViableEpochDescriptor<Hash, Number, E> {
	/// Start slot of the descriptor.
	pub fn start_slot(&self) -> E::Slot {
		match self {
			Self::UnimportedGenesis(start_slot) => *start_slot,
			Self::Signaled(_, header) => header.start_slot,
		}
	}
}

/// Persisted epoch stored in EpochChanges.
#[derive(Clone, Encode, Decode, Debug)]
pub enum PersistedEpoch<E> {
	/// Genesis persisted epoch data. epoch_0, epoch_1.
	Genesis(E, E),
	/// Regular persisted epoch data. epoch_n.
	Regular(E),
}

impl<E> PersistedEpoch<E> {
	/// Returns if this is a genesis epoch.
	pub fn is_genesis(&self) -> bool {
		matches!(self, Self::Genesis(_, _))
	}
}

impl<'a, E: Epoch> From<&'a PersistedEpoch<E>> for PersistedEpochHeader<E> {
	fn from(epoch: &'a PersistedEpoch<E>) -> Self {
		match epoch {
			PersistedEpoch::Genesis(ref epoch_0, ref epoch_1) => {
				PersistedEpochHeader::Genesis(epoch_0.into(), epoch_1.into())
			},
			PersistedEpoch::Regular(ref epoch_n) => PersistedEpochHeader::Regular(epoch_n.into()),
		}
	}
}

impl<E: Epoch> PersistedEpoch<E> {
	/// Map the epoch to a different type using a conversion function.
	pub fn map<B, F, Hash, Number>(self, h: &Hash, n: &Number, f: &mut F) -> PersistedEpoch<B>
	where
		B: Epoch<Slot = E::Slot>,
		F: FnMut(&Hash, &Number, E) -> B,
	{
		match self {
			PersistedEpoch::Genesis(epoch_0, epoch_1) => {
				PersistedEpoch::Genesis(f(h, n, epoch_0), f(h, n, epoch_1))
			},
			PersistedEpoch::Regular(epoch_n) => PersistedEpoch::Regular(f(h, n, epoch_n)),
		}
	}
}

/// Persisted epoch header stored in ForkTree.
#[derive(Encode, Decode, PartialEq, Eq, Debug)]
pub enum PersistedEpochHeader<E: Epoch> {
	/// Genesis persisted epoch header. epoch_0, epoch_1.
	Genesis(EpochHeader<E>, EpochHeader<E>),
	/// Regular persisted epoch header. epoch_n.
	Regular(EpochHeader<E>),
}

impl<E: Epoch> Clone for PersistedEpochHeader<E> {
	fn clone(&self) -> Self {
		match self {
			Self::Genesis(epoch_0, epoch_1) => Self::Genesis(epoch_0.clone(), epoch_1.clone()),
			Self::Regular(epoch_n) => Self::Regular(epoch_n.clone()),
		}
	}
}

impl<E: Epoch> PersistedEpochHeader<E> {
	/// Map the epoch header to a different type.
	pub fn map<B>(self) -> PersistedEpochHeader<B>
	where
		B: Epoch<Slot = E::Slot>,
	{
		match self {
			PersistedEpochHeader::Genesis(epoch_0, epoch_1) => PersistedEpochHeader::Genesis(
				EpochHeader { start_slot: epoch_0.start_slot, end_slot: epoch_0.end_slot },
				EpochHeader { start_slot: epoch_1.start_slot, end_slot: epoch_1.end_slot },
			),
			PersistedEpochHeader::Regular(epoch_n) => PersistedEpochHeader::Regular(EpochHeader {
				start_slot: epoch_n.start_slot,
				end_slot: epoch_n.end_slot,
			}),
		}
	}
}

/// A fresh, incremented epoch to import into the underlying fork-tree.
///
/// Create this with `ViableEpoch::increment`.
#[must_use = "Freshly-incremented epoch must be imported with `EpochChanges::import`"]
pub struct IncrementedEpoch<E: Epoch>(PersistedEpoch<E>);

impl<E: Epoch> AsRef<E> for IncrementedEpoch<E> {
	fn as_ref(&self) -> &E {
		match self.0 {
			PersistedEpoch::Genesis(_, ref epoch_1) => epoch_1,
			PersistedEpoch::Regular(ref epoch_n) => epoch_n,
		}
	}
}

/// Tree of all epoch changes across all *seen* forks. Data stored in tree is
/// the hash and block number of the block signaling the epoch change, and the
/// epoch that was signalled at that block.
///
/// The first epoch, epoch_0, is special cased by saying that it starts at
/// slot number of the first block in the chain. When bootstrapping a chain,
/// there can be multiple competing block #1s, so we have to ensure that the overlaid
/// DAG doesn't get confused.
///
/// The first block of every epoch should be producing a descriptor for the next
/// epoch - this is checked in higher-level code. So the first block of epoch_0 contains
/// a descriptor for epoch_1. We special-case these and bundle them together in the
/// same DAG entry, pinned to a specific block #1.
///
/// Further epochs (epoch_2, ..., epoch_n) each get their own entry.
#[derive(Clone, Encode, Decode, Debug)]
pub struct EpochChanges<Hash, Number, E: Epoch> {
	inner: ForkTree<Hash, Number, PersistedEpochHeader<E>>,
	epochs: BTreeMap<(Hash, Number), PersistedEpoch<E>>,
}

// create a fake header hash which hasn't been included in the chain.
fn fake_head_hash<H: AsRef<[u8]> + AsMut<[u8]> + Clone>(parent_hash: &H) -> H {
	let mut h = parent_hash.clone();
	// dirty trick: flip the first bit of the parent hash to create a hash
	// which has not been in the chain before (assuming a strong hash function).
	h.as_mut()[0] ^= 0b10000000;
	h
}

impl<Hash, Number, E: Epoch> Default for EpochChanges<Hash, Number, E>
where
	Hash: PartialEq + Ord,
	Number: Ord,
{
	fn default() -> Self {
		EpochChanges { inner: ForkTree::new(), epochs: BTreeMap::new() }
	}
}

impl<Hash, Number, E: Epoch> EpochChanges<Hash, Number, E>
where
	Hash: PartialEq + Ord + AsRef<[u8]> + AsMut<[u8]> + Copy + std::fmt::Debug,
	Number: Ord + One + Zero + Add<Output = Number> + Sub<Output = Number> + Copy + std::fmt::Debug,
{
	/// Create a new epoch change.
	pub fn new() -> Self {
		Self::default()
	}

	/// Rebalances the tree of epoch changes so that it is sorted by length of
	/// fork (longest fork first).
	pub fn rebalance(&mut self) {
		self.inner.rebalance()
	}

	/// Map the epoch changes from one storing data to a different one.
	pub fn map<B, F>(self, mut f: F) -> EpochChanges<Hash, Number, B>
	where
		B: Epoch<Slot = E::Slot>,
		F: FnMut(&Hash, &Number, E) -> B,
	{
		EpochChanges {
			inner: self.inner.map(&mut |_, _, header: PersistedEpochHeader<E>| header.map()),
			epochs: self
				.epochs
				.into_iter()
				.map(|((hash, number), epoch)| ((hash, number), epoch.map(&hash, &number, &mut f)))
				.collect(),
		}
	}

	/// Prune out finalized epochs, except for the ancestor of the finalized
	/// block. The given slot should be the slot number at which the finalized
	/// block was authored.
	pub fn prune_finalized<D: IsDescendentOfBuilder<Hash>>(
		&mut self,
		descendent_of_builder: D,
		hash: &Hash,
		number: Number,
		slot: E::Slot,
	) -> Result<(), fork_tree::Error<D::Error>> {
		let is_descendent_of = descendent_of_builder.build_is_descendent_of(None);

		let predicate = |epoch: &PersistedEpochHeader<E>| match *epoch {
			PersistedEpochHeader::Genesis(_, ref epoch_1) => epoch_1.start_slot <= slot,
			PersistedEpochHeader::Regular(ref epoch_n) => epoch_n.start_slot <= slot,
		};

		// Prune any epochs which could not be _live_ as of the children of the
		// finalized block, i.e. re-root the fork tree to the oldest ancestor of
		// (hash, number) where `epoch.start_slot() <= finalized_slot`.
		let removed = self.inner.prune(hash, &number, &is_descendent_of, &predicate)?;

		for (hash, number, _) in removed {
			self.epochs.remove(&(hash, number));
		}

		Ok(())
	}

	/// Get a reference to an epoch with given identifier.
	pub fn epoch(&self, id: &EpochIdentifier<Hash, Number>) -> Option<&E> {
		self.epochs.get(&(id.hash, id.number)).and_then(|v| match v {
			PersistedEpoch::Genesis(ref epoch_0, _)
				if id.position == EpochIdentifierPosition::Genesis0 =>
			{
				Some(epoch_0)
			},
			PersistedEpoch::Genesis(_, ref epoch_1)
				if id.position == EpochIdentifierPosition::Genesis1 =>
			{
				Some(epoch_1)
			},
			PersistedEpoch::Regular(ref epoch_n)
				if id.position == EpochIdentifierPosition::Regular =>
			{
				Some(epoch_n)
			},
			_ => None,
		})
	}

	/// Get a reference to a viable epoch with given descriptor.
	pub fn viable_epoch<G>(
		&self,
		descriptor: &ViableEpochDescriptor<Hash, Number, E>,
		make_genesis: G,
	) -> Option<ViableEpoch<E, &E>>
	where
		G: FnOnce(E::Slot) -> E,
	{
		match descriptor {
			ViableEpochDescriptor::UnimportedGenesis(slot) => {
				Some(ViableEpoch::UnimportedGenesis(make_genesis(*slot)))
			},
			ViableEpochDescriptor::Signaled(identifier, _) => {
				self.epoch(identifier).map(ViableEpoch::Signaled)
			},
		}
	}

	/// Get a mutable reference to an epoch with given identifier.
	pub fn epoch_mut(&mut self, id: &EpochIdentifier<Hash, Number>) -> Option<&mut E> {
		self.epochs.get_mut(&(id.hash, id.number)).and_then(|v| match v {
			PersistedEpoch::Genesis(ref mut epoch_0, _)
				if id.position == EpochIdentifierPosition::Genesis0 =>
			{
				Some(epoch_0)
			},
			PersistedEpoch::Genesis(_, ref mut epoch_1)
				if id.position == EpochIdentifierPosition::Genesis1 =>
			{
				Some(epoch_1)
			},
			PersistedEpoch::Regular(ref mut epoch_n)
				if id.position == EpochIdentifierPosition::Regular =>
			{
				Some(epoch_n)
			},
			_ => None,
		})
	}

	/// Get a mutable reference to a viable epoch with given descriptor.
	pub fn viable_epoch_mut<G>(
		&mut self,
		descriptor: &ViableEpochDescriptor<Hash, Number, E>,
		make_genesis: G,
	) -> Option<ViableEpoch<E, &mut E>>
	where
		G: FnOnce(E::Slot) -> E,
	{
		match descriptor {
			ViableEpochDescriptor::UnimportedGenesis(slot) => {
				Some(ViableEpoch::UnimportedGenesis(make_genesis(*slot)))
			},
			ViableEpochDescriptor::Signaled(identifier, _) => {
				self.epoch_mut(identifier).map(ViableEpoch::Signaled)
			},
		}
	}

	/// Get the epoch data from an epoch descriptor.
	///
	/// Note that this function ignores the fact that an genesis epoch might need to be imported.
	/// Mostly useful for testing.
	pub fn epoch_data<G>(
		&self,
		descriptor: &ViableEpochDescriptor<Hash, Number, E>,
		make_genesis: G,
	) -> Option<E>
	where
		G: FnOnce(E::Slot) -> E,
		E: Clone,
	{
		match descriptor {
			ViableEpochDescriptor::UnimportedGenesis(slot) => Some(make_genesis(*slot)),
			ViableEpochDescriptor::Signaled(identifier, _) => self.epoch(identifier).cloned(),
		}
	}

	/// Finds the epoch data for a child of the given block. Similar to
	/// `epoch_descriptor_for_child_of` but returns the full data.
	///
	/// Note that this function ignores the fact that an genesis epoch might need to be imported.
	/// Mostly useful for testing.
	pub fn epoch_data_for_child_of<D: IsDescendentOfBuilder<Hash>, G>(
		&self,
		descendent_of_builder: D,
		parent_hash: &Hash,
		parent_number: Number,
		slot: E::Slot,
		make_genesis: G,
	) -> Result<Option<E>, fork_tree::Error<D::Error>>
	where
		G: FnOnce(E::Slot) -> E,
		E: Clone,
	{
		let descriptor = self.epoch_descriptor_for_child_of(
			descendent_of_builder,
			parent_hash,
			parent_number,
			slot,
		)?;

		Ok(descriptor.and_then(|des| self.epoch_data(&des, make_genesis)))
	}

	/// Finds the epoch for a child of the given block, assuming the given slot number.
	///
	/// If the returned epoch is an `UnimportedGenesis` epoch, it should be imported into the
	/// tree.
	pub fn epoch_descriptor_for_child_of<D: IsDescendentOfBuilder<Hash>>(
		&self,
		descendent_of_builder: D,
		parent_hash: &Hash,
		parent_number: Number,
		slot: E::Slot,
	) -> Result<Option<ViableEpochDescriptor<Hash, Number, E>>, fork_tree::Error<D::Error>> {
		if parent_number == Zero::zero() {
			// need to insert the genesis epoch.
			return Ok(Some(ViableEpochDescriptor::UnimportedGenesis(slot)));
		}

		// find_node_where will give you the node in the fork-tree which is an ancestor
		// of the `parent_hash` by default. if the last epoch was signalled at the parent_hash,
		// then it won't be returned. we need to create a new fake chain head hash which
		// "descends" from our parent-hash.
		let fake_head_hash = fake_head_hash(parent_hash);

		let is_descendent_of =
			descendent_of_builder.build_is_descendent_of(Some((fake_head_hash, *parent_hash)));

		// We want to find the deepest node in the tree which is an ancestor
		// of our block and where the start slot of the epoch was before the
		// slot of our block. The genesis special-case doesn't need to look
		// at epoch_1 -- all we're doing here is figuring out which node
		// we need.
		let predicate = |epoch: &PersistedEpochHeader<E>| match *epoch {
			PersistedEpochHeader::Genesis(ref epoch_0, _) => epoch_0.start_slot <= slot,
			PersistedEpochHeader::Regular(ref epoch_n) => epoch_n.start_slot <= slot,
		};

		self.inner
			.find_node_where(
				&fake_head_hash,
				&(parent_number + One::one()),
				&is_descendent_of,
				&predicate,
			)
			.map(|n| {
				n.map(|node| {
					(
						match node.data {
							// Ok, we found our node.
							// and here we figure out which of the internal epochs
							// of a genesis node to use based on their start slot.
							PersistedEpochHeader::Genesis(ref epoch_0, ref epoch_1) => {
								if epoch_1.start_slot <= slot {
									(EpochIdentifierPosition::Genesis1, epoch_1.clone())
								} else {
									(EpochIdentifierPosition::Genesis0, epoch_0.clone())
								}
							},
							PersistedEpochHeader::Regular(ref epoch_n) => {
								(EpochIdentifierPosition::Regular, epoch_n.clone())
							},
						},
						node,
					)
				})
				.map(|((position, header), node)| {
					ViableEpochDescriptor::Signaled(
						EpochIdentifier { position, hash: node.hash, number: node.number },
						header,
					)
				})
			})
	}

	/// Import a new epoch-change, signalled at the given block.
	///
	/// This assumes that the given block is prospective (i.e. has not been
	/// imported yet), but its parent has. This is why the parent hash needs
	/// to be provided.
	pub fn import<D: IsDescendentOfBuilder<Hash>>(
		&mut self,
		descendent_of_builder: D,
		hash: Hash,
		number: Number,
		parent_hash: Hash,
		epoch: IncrementedEpoch<E>,
	) -> Result<(), fork_tree::Error<D::Error>> {
		let is_descendent_of =
			descendent_of_builder.build_is_descendent_of(Some((hash, parent_hash)));
		let IncrementedEpoch(epoch) = epoch;
		let header = PersistedEpochHeader::<E>::from(&epoch);

		let res = self.inner.import(hash, number, header, &is_descendent_of);

		match res {
			Ok(_) | Err(fork_tree::Error::Duplicate) => {
				self.epochs.insert((hash, number), epoch);
				Ok(())
			},
			Err(e) => Err(e),
		}
	}

	/// Reset to a specified pair of epochs, as if they were announced at blocks `parent_hash` and
	/// `hash`.
	pub fn reset(&mut self, parent_hash: Hash, hash: Hash, number: Number, current: E, next: E) {
		self.inner = ForkTree::new();
		self.epochs.clear();
		let persisted = PersistedEpoch::Regular(current);
		let header = PersistedEpochHeader::from(&persisted);
		let _res = self.inner.import(parent_hash, number - One::one(), header, &|_, _| {
			Ok(false) as Result<bool, fork_tree::Error<ClientError>>
		});
		self.epochs.insert((parent_hash, number - One::one()), persisted);

		let persisted = PersistedEpoch::Regular(next);
		let header = PersistedEpochHeader::from(&persisted);
		let _res = self.inner.import(hash, number, header, &|_, _| {
			Ok(true) as Result<bool, fork_tree::Error<ClientError>>
		});
		self.epochs.insert((hash, number), persisted);
	}

	/// Revert to a specified block given its `hash` and `number`.
	/// This removes all the epoch changes information that were announced by
	/// all the given block descendants.
	pub fn revert<D: IsDescendentOfBuilder<Hash>>(
		&mut self,
		descendent_of_builder: D,
		hash: Hash,
		number: Number,
	) {
		let is_descendent_of = descendent_of_builder.build_is_descendent_of(None);

		let filter = |node_hash: &Hash, node_num: &Number, _: &PersistedEpochHeader<E>| {
			if number >= *node_num &&
				(is_descendent_of(node_hash, &hash).unwrap_or_default() || *node_hash == hash)
			{
				// Continue the search in this subtree.
				FilterAction::KeepNode
			} else if number < *node_num && is_descendent_of(&hash, node_hash).unwrap_or_default() {
				// Found a node to be removed.
				FilterAction::Remove
			} else {
				// Not a parent or child of the one we're looking for, stop processing this branch.
				FilterAction::KeepTree
			}
		};

		self.inner.drain_filter(filter).for_each(|(h, n, _)| {
			self.epochs.remove(&(h, n));
		});
	}

	/// Return the inner fork tree (mostly useful for testing)
	pub fn tree(&self) -> &ForkTree<Hash, Number, PersistedEpochHeader<E>> {
		&self.inner
	}
}

/// Type alias to produce the epoch-changes tree from a block type.
pub type EpochChangesFor<Block, Epoch> =
	EpochChanges<<Block as BlockT>::Hash, NumberFor<Block>, Epoch>;

/// A shared epoch changes tree.
pub type SharedEpochChanges<Block, Epoch> =
	sc_consensus::shared_data::SharedData<EpochChangesFor<Block, Epoch>>;

#[cfg(test)]
mod tests {
}
