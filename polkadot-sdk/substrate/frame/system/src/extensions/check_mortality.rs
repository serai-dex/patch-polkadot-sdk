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

use crate::{pallet_prelude::BlockNumberFor, BlockHash, Config, Pallet};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::TransactionSource;
use sp_runtime::{
	generic::Era,
	impl_tx_ext_default,
	traits::{DispatchInfoOf, SaturatedConversion, TransactionExtension, ValidateResult},
	transaction_validity::{InvalidTransaction, TransactionValidityError, ValidTransaction},
};

/// Check for transaction mortality.
///
/// The extension adds [`Era`] to every signed extrinsic. It also contributes to the signed data, by
/// including the hash of the block at [`Era::birth`].
///
/// # Transaction Validity
///
/// The extension affects `longevity` of the transaction according to the [`Era`] definition.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq)]
pub struct CheckMortality<T: Config + Send + Sync>(pub Era, core::marker::PhantomData<T>);

impl<T: Config + Send + Sync> CheckMortality<T> {
	/// utility constructor. Used only in client/factory code.
	pub fn from(era: Era) -> Self {
		Self(era, core::marker::PhantomData)
	}
}

impl<T: Config + Send + Sync> core::fmt::Debug for CheckMortality<T> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		write!(f, "CheckMortality({:?})", self.0)
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
		Ok(())
	}
}

impl<T: Config + Send + Sync> TransactionExtension<T::RuntimeCall> for CheckMortality<T> {
	const IDENTIFIER: &'static str = "CheckMortality";
	type Implicit = T::Hash;

	fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
		let current_u64 = <Pallet<T>>::block_number().saturated_into::<u64>();
		let n = self.0.birth(current_u64).saturated_into::<BlockNumberFor<T>>();
		if !<BlockHash<T>>::contains_key(n) {
			Err(InvalidTransaction::AncientBirthBlock.into())
		} else {
			Ok(<Pallet<T>>::block_hash(n))
		}
	}
	type Pre = ();
	type Val = ();

	fn weight(&self, _: &T::RuntimeCall) -> sp_weights::Weight {
		if self.0.is_immortal() {
			// All immortal transactions will always read the hash of the genesis block, so to avoid
			// charging this multiple times in a block we manually set the proof size to 0.
			<T::ExtensionsWeightInfo as super::WeightInfo>::check_mortality_immortal_transaction()
				.set_proof_size(0)
		} else {
			<T::ExtensionsWeightInfo as super::WeightInfo>::check_mortality_mortal_transaction()
		}
	}

	fn validate(
		&self,
		origin: <T as Config>::RuntimeOrigin,
		_call: &T::RuntimeCall,
		_info: &DispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl Encode,
		_source: TransactionSource,
	) -> ValidateResult<Self::Val, T::RuntimeCall> {
		let current_u64 = <Pallet<T>>::block_number().saturated_into::<u64>();
		let valid_till = self.0.death(current_u64);
		Ok((
			ValidTransaction {
				longevity: valid_till.saturating_sub(current_u64),
				..Default::default()
			},
			(),
			origin,
		))
	}
	impl_tx_ext_default!(T::RuntimeCall; prepare);
}

#[cfg(test)]
mod tests {
}
