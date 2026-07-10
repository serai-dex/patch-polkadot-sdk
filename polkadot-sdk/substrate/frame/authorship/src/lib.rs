// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Authorship tracking for FRAME runtimes.
//!
//! This tracks the current author of the block.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::traits::FindAuthor;

pub use pallet::*;

/// An event handler for the authorship pallet. There is a dummy implementation
/// for `()`, which does nothing.
#[impl_trait_for_tuples::impl_for_tuples(30)]
pub trait EventHandler<Author, BlockNumber> {
	/// Note that the given account ID is the author of the current block.
	fn note_author(author: Author);
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Find the author of a block.
		type FindAuthor: FindAuthor<Self::AccountId>;
		/// An event handler for authored blocks.
		type EventHandler: EventHandler<Self::AccountId, BlockNumberFor<Self>>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_: BlockNumberFor<T>) -> Weight {
			if let Some(author) = Self::author() {
				T::EventHandler::note_author(author);
			}

			Weight::zero()
		}

		fn on_finalize(_: BlockNumberFor<T>) {
			// ensure we never go to trie with these values.
			<Author<T>>::kill();
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			Self::do_try_state()
		}
	}

	#[pallet::storage]
	#[pallet::whitelist_storage]
	/// Author of current block.
	pub(super) type Author<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
}

impl<T: Config> Pallet<T> {
	/// Fetch the author of the block.
	///
	/// This is safe to invoke in `on_initialize` implementations, as well
	/// as afterwards.
	pub fn author() -> Option<T::AccountId> {
		// Check the memorized storage value.
		if let Some(author) = <Author<T>>::get() {
			return Some(author);
		}

		let digest = <frame_system::Pallet<T>>::digest();
		let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
		T::FindAuthor::find_author(pre_runtime_digests).inspect(|a| {
			<Author<T>>::put(&a);
		})
	}
}

#[cfg(any(feature = "try-runtime", test))]
impl<T: Config> Pallet<T> {
	/// Ensure the correctness of the state of this pallet.
	///
	/// # Invariants
	///
	/// * If `Author` storage contains a value, it must match the author derived from the current
	///   block's digest via `FindAuthor`.
	pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
		use frame_support::ensure;

		if let Some(stored_author) = <Author<T>>::get() {
			let digest = <frame_system::Pallet<T>>::digest();
			let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
			if let Some(expected_author) = T::FindAuthor::find_author(pre_runtime_digests) {
				ensure!(
					stored_author == expected_author,
					"Stored author does not match the author derived from digest"
				);
			}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
}
