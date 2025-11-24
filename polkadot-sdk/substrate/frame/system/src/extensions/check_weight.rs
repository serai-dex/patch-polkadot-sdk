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

use crate::{limits::BlockWeights, Config, Pallet, LOG_TARGET};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
	dispatch::{DispatchInfo, PostDispatchInfo},
	pallet_prelude::TransactionSource,
	traits::Get,
};
use sp_runtime::{
	traits::{
		DispatchInfoOf, Dispatchable, PostDispatchInfoOf, TransactionExtension, ValidateResult,
	},
	transaction_validity::{InvalidTransaction, TransactionValidityError, ValidTransaction},
	DispatchResult,
};
use sp_weights::Weight;

/// Block resource (weight) limit check.
///
/// # Transaction Validity
///
/// This extension does not influence any fields of `TransactionValidity` in case the
/// transaction is valid.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, Default)]
pub struct CheckWeight<T: Config + Send + Sync>(core::marker::PhantomData<T>);

impl<T: Config + Send + Sync> CheckWeight<T>
where
	T::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
{
	/// Checks if the current extrinsic does not exceed the maximum weight a single extrinsic
	/// with given `DispatchClass` can have.
	fn check_extrinsic_weight(
		info: &DispatchInfoOf<T::RuntimeCall>,
	) -> Result<(), TransactionValidityError> {
		let max = T::BlockWeights::get().get(info.class).max_extrinsic;
		match max {
			Some(max) if info.total_weight().any_gt(max) => {
				log::debug!(
					target: LOG_TARGET,
					"Extrinsic {} is greater than the max extrinsic {}",
					info.total_weight(),
					max,
				);

				Err(InvalidTransaction::ExhaustsResources.into())
			},
			_ => Ok(()),
		}
	}

	/// Checks if the current extrinsic can fit into the block with respect to block length limits.
	///
	/// Upon successes, it returns the new block length as a `Result`.
	fn check_block_length(
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Result<u32, TransactionValidityError> {
		let length_limit = T::BlockLength::get();
		let current_len = Pallet::<T>::all_extrinsics_len();
		let added_len = len as u32;
		let next_len = current_len.saturating_add(added_len);
		if next_len > *length_limit.max.get(info.class) {
			log::debug!(
				target: LOG_TARGET,
				"Exceeded block length limit: {} > {}",
				next_len,
				length_limit.max.get(info.class),
			);

			Err(InvalidTransaction::ExhaustsResources.into())
		} else {
			Ok(next_len)
		}
	}

	/// Creates new `TransactionExtension` to check weight of the extrinsic.
	pub fn new() -> Self {
		Self(Default::default())
	}

	/// Do the validate checks. This can be applied to both signed and unsigned.
	///
	/// It only checks that the block weight and length limit will not exceed.
	///
	/// Returns the transaction validity and the next block length, to be used in `prepare`.
	pub fn do_validate(
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Result<(ValidTransaction, u32), TransactionValidityError> {
		// If they return `Ok`, then it is below the limit.
		let next_len = Self::check_block_length(info, len)?;
		// during validation we skip block limit check. Since the `validate_transaction`
		// call runs on an empty block anyway, by this we prevent `on_initialize` weight
		// consumption from causing false negatives.
		Self::check_extrinsic_weight(info)?;

		Ok((Default::default(), next_len))
	}

	/// Do the pre-dispatch checks. This can be applied to both signed and unsigned.
	///
	/// It checks and notes the new weight and length.
	pub fn do_prepare(
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
		next_len: u32,
	) -> Result<(), TransactionValidityError> {
		let all_weight = Pallet::<T>::block_weight();
		let maximum_weight = T::BlockWeights::get();
		let next_weight =
			calculate_consumed_weight::<T::RuntimeCall>(&maximum_weight, all_weight, info, len)?;
		// Extrinsic weight already checked in `validate`.

		crate::AllExtrinsicsLen::<T>::put(next_len);
		crate::BlockWeight::<T>::put(next_weight);
		Ok(())
	}

	#[deprecated(note = "Use `frame_system::Pallet::reclaim_weight` instead.")]
	pub fn do_post_dispatch(
		info: &DispatchInfoOf<T::RuntimeCall>,
		post_info: &PostDispatchInfoOf<T::RuntimeCall>,
	) -> Result<(), TransactionValidityError> {
		crate::Pallet::<T>::reclaim_weight(info, post_info)
	}
}

/// Checks if the current extrinsic can fit into the block with respect to block weight limits.
///
/// Upon successes, it returns the new block weight as a `Result`.
pub fn calculate_consumed_weight<Call>(
	maximum_weight: &BlockWeights,
	mut all_weight: crate::ConsumedWeight,
	info: &DispatchInfoOf<Call>,
	len: usize,
) -> Result<crate::ConsumedWeight, TransactionValidityError>
where
	Call: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
{
	// Also Consider extrinsic length as proof weight.
	let extrinsic_weight = info
		.total_weight()
		.saturating_add(maximum_weight.get(info.class).base_extrinsic)
		.saturating_add(Weight::from_parts(0, len as u64));
	let limit_per_class = maximum_weight.get(info.class);

	// add the weight. If class is unlimited, use saturating add instead of checked one.
	if limit_per_class.max_total.is_none() && limit_per_class.reserved.is_none() {
		all_weight.accrue(extrinsic_weight, info.class)
	} else {
		all_weight.checked_accrue(extrinsic_weight, info.class).map_err(|_| {
			log::debug!(
				target: LOG_TARGET,
				"All weight checked add overflow.",
			);

			InvalidTransaction::ExhaustsResources
		})?;
	}

	let per_class = *all_weight.get(info.class);

	// Check if we don't exceed per-class allowance
	match limit_per_class.max_total {
		Some(max) if per_class.any_gt(max) => {
			log::debug!(
				target: LOG_TARGET,
				"Exceeded the per-class allowance.",
			);

			return Err(InvalidTransaction::ExhaustsResources.into());
		},
		// There is no `max_total` limit (`None`),
		// or we are below the limit.
		_ => {},
	}

	// In cases total block weight is exceeded, we need to fall back
	// to `reserved` pool if there is any.
	if all_weight.total().any_gt(maximum_weight.max_block) {
		match limit_per_class.reserved {
			// We are over the limit in reserved pool.
			Some(reserved) if per_class.any_gt(reserved) => {
				log::debug!(
					target: LOG_TARGET,
					"Total block weight is exceeded.",
				);

				return Err(InvalidTransaction::ExhaustsResources.into());
			},
			// There is either no limit in reserved pool (`None`),
			// or we are below the limit.
			_ => {},
		}
	}

	Ok(all_weight)
}

impl<T: Config + Send + Sync> TransactionExtension<T::RuntimeCall> for CheckWeight<T>
where
	T::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
{
	const IDENTIFIER: &'static str = "CheckWeight";
	type Implicit = ();
	type Pre = ();
	type Val = u32; /* next block length */

	fn weight(&self, _: &T::RuntimeCall) -> Weight {
		<T::ExtensionsWeightInfo as super::WeightInfo>::check_weight()
	}

	fn validate(
		&self,
		origin: T::RuntimeOrigin,
		_call: &T::RuntimeCall,
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl Encode,
		_source: TransactionSource,
	) -> ValidateResult<Self::Val, T::RuntimeCall> {
		let (validity, next_len) = Self::do_validate(info, len)?;
		Ok((validity, next_len, origin))
	}

	fn prepare(
		self,
		val: Self::Val,
		_origin: &T::RuntimeOrigin,
		_call: &T::RuntimeCall,
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		Self::do_prepare(info, len, val)
	}

	fn post_dispatch_details(
		_pre: Self::Pre,
		info: &DispatchInfoOf<T::RuntimeCall>,
		post_info: &PostDispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<Weight, TransactionValidityError> {
		crate::Pallet::<T>::reclaim_weight(info, post_info).map(|()| Weight::zero())
	}

	fn bare_validate(
		_call: &T::RuntimeCall,
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> frame_support::pallet_prelude::TransactionValidity {
		Ok(Self::do_validate(info, len)?.0)
	}

	fn bare_validate_and_prepare(
		_call: &T::RuntimeCall,
		info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Result<(), TransactionValidityError> {
		let (_, next_len) = Self::do_validate(info, len)?;
		Self::do_prepare(info, len, next_len)
	}

	fn bare_post_dispatch(
		info: &DispatchInfoOf<T::RuntimeCall>,
		post_info: &mut PostDispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<(), TransactionValidityError> {
		crate::Pallet::<T>::reclaim_weight(info, post_info)
	}
}

impl<T: Config + Send + Sync> core::fmt::Debug for CheckWeight<T> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		write!(f, "CheckWeight")
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
}
