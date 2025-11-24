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

use crate::Config;
use frame_support::{
	dispatch::DispatchInfo,
	pallet_prelude::{
		Decode, DecodeWithMemTracking, DispatchResult, Encode, TransactionSource, Weight,
	},
	traits::Authorize,
	CloneNoBound, EqNoBound, PartialEqNoBound, RuntimeDebugNoBound,
};
use sp_runtime::{
	traits::{
		AsTransactionAuthorizedOrigin, Dispatchable, Implication, PostDispatchInfoOf,
		TransactionExtension, ValidateResult,
	},
	transaction_validity::TransactionValidityError,
};

/// A transaction extension that authorizes some calls (i.e. dispatchable functions) to be
/// included in the block.
///
/// This transaction extension use the runtime implementation of the trait
/// [`Authorize`](frame_support::traits::Authorize) to set the validity of the transaction.
#[derive(
	Encode,
	Decode,
	CloneNoBound,
	EqNoBound,
	PartialEqNoBound,

	RuntimeDebugNoBound,
	DecodeWithMemTracking,
)]
pub struct AuthorizeCall<T>(core::marker::PhantomData<T>);

impl<T> AuthorizeCall<T> {
	pub fn new() -> Self {
		Self(Default::default())
	}
}

impl<T: Config + Send + Sync> TransactionExtension<T::RuntimeCall> for AuthorizeCall<T>
where
	T::RuntimeCall: Dispatchable<Info = DispatchInfo>,
{
	const IDENTIFIER: &'static str = "AuthorizeCall";
	type Implicit = ();
	type Val = Weight;
	type Pre = Weight;

	fn validate(
		&self,
		origin: T::RuntimeOrigin,
		call: &T::RuntimeCall,
		_info: &DispatchInfo,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl Implication,
		source: TransactionSource,
	) -> ValidateResult<Self::Val, T::RuntimeCall> {
		if !origin.is_transaction_authorized() {
			if let Some(authorize) = call.authorize(source) {
				return authorize.map(|(validity, unspent)| {
					(validity, unspent, crate::Origin::<T>::Authorized.into())
				})
			}
		}

		Ok((Default::default(), Weight::zero(), origin))
	}

	fn prepare(
		self,
		val: Self::Val,
		_origin: &T::RuntimeOrigin,
		_call: &T::RuntimeCall,
		_info: &DispatchInfo,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		Ok(val)
	}

	fn post_dispatch_details(
		pre: Self::Pre,
		_info: &DispatchInfo,
		_post_info: &PostDispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<Weight, TransactionValidityError> {
		Ok(pre)
	}

	fn weight(&self, call: &T::RuntimeCall) -> Weight {
		call.weight_of_authorize()
	}
}

#[cfg(test)]
mod tests {
}
