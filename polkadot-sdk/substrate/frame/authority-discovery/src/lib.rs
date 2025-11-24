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

//! # Authority discovery pallet.
//!
//! This pallet is used by the `client/authority-discovery` and by polkadot's parachain logic
//! to retrieve the current and the next set of authorities.

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use frame_support::{
	traits::{Get, OneSessionHandler},
	WeakBoundedVec,
};
use sp_authority_discovery::AuthorityId;

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The pallet's config trait.
	pub trait Config: frame_system::Config + pallet_session::Config {
		/// The maximum number of authorities that can be added.
		type MaxAuthorities: Get<u32>;
	}

	#[pallet::storage]
	/// Keys of the current authority set.
	pub type Keys<T: Config> =
		StorageValue<_, WeakBoundedVec<AuthorityId, T::MaxAuthorities>, ValueQuery>;

	#[pallet::storage]
	/// Keys of the next authority set.
	pub type NextKeys<T: Config> =
		StorageValue<_, WeakBoundedVec<AuthorityId, T::MaxAuthorities>, ValueQuery>;

	#[derive(frame_support::DefaultNoBound)]
	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub keys: Vec<AuthorityId>,
		#[codec(skip)]
		pub _config: core::marker::PhantomData<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			Pallet::<T>::initialize_keys(&self.keys)
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Retrieve authority identifiers of the current and next authority set
	/// sorted and deduplicated.
	pub fn authorities() -> Vec<AuthorityId> {
		let mut keys = Keys::<T>::get().to_vec();
		let next = NextKeys::<T>::get().to_vec();

		keys.extend(next);
		keys.sort();
		keys.dedup();

		keys.to_vec()
	}

	/// Retrieve authority identifiers of the current authority set in the original order.
	pub fn current_authorities() -> WeakBoundedVec<AuthorityId, T::MaxAuthorities> {
		Keys::<T>::get()
	}

	/// Retrieve authority identifiers of the next authority set in the original order.
	pub fn next_authorities() -> WeakBoundedVec<AuthorityId, T::MaxAuthorities> {
		NextKeys::<T>::get()
	}

	fn initialize_keys(keys: &Vec<AuthorityId>) {
		if !keys.is_empty() {
			assert!(Keys::<T>::get().is_empty(), "Keys are already initialized!");

			let bounded_keys =
				WeakBoundedVec::<AuthorityId, T::MaxAuthorities>::try_from((*keys).clone())
					.expect("Keys vec too big");

			Keys::<T>::put(&bounded_keys);
			NextKeys::<T>::put(&bounded_keys);
		}
	}
}

impl<T: Config> sp_runtime::BoundToRuntimeAppPublic for Pallet<T> {
	type Public = AuthorityId;
}

impl<T: Config> OneSessionHandler<T::AccountId> for Pallet<T> {
	type Key = AuthorityId;

	fn on_genesis_session<'a, I: 'a>(authorities: I)
	where
		I: Iterator<Item = (&'a T::AccountId, Self::Key)>,
	{
		Self::initialize_keys(&authorities.map(|x| x.1).collect::<Vec<_>>());
	}

	fn on_new_session<'a, I: 'a>(changed: bool, validators: I, queued_validators: I)
	where
		I: Iterator<Item = (&'a T::AccountId, Self::Key)>,
	{
		// Remember who the authorities are for the new and next session.
		if changed {
			let keys = validators.map(|x| x.1).collect::<Vec<_>>();

			let bounded_keys = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
				keys,
				Some(
					"Warning: The session has more validators than expected. \
				A runtime configuration adjustment may be needed.",
				),
			);

			Keys::<T>::put(bounded_keys);
		}

		// `changed` represents if queued_validators changed in the previous session not in the
		// current one.
		let next_keys = queued_validators.map(|x| x.1).collect::<Vec<_>>();

		let next_bounded_keys = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
			next_keys,
			Some(
				"Warning: The session has more queued validators than expected. \
			A runtime configuration adjustment may be needed.",
			),
		);

		NextKeys::<T>::put(next_bounded_keys);
	}

	fn on_disabled(_i: u32) {
		// ignore
	}
}

#[cfg(test)]
mod tests {
}
