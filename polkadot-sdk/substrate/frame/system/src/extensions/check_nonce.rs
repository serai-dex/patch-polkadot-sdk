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

extern crate alloc;

use alloc::{vec, vec::Vec};

use crate::Config;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
	dispatch::DispatchInfo, pallet_prelude::TransactionSource, RuntimeDebugNoBound,
};
use sp_runtime::{
	traits::{
		AsSystemOriginSigner, CheckedAdd, DispatchInfoOf, Dispatchable, One, PostDispatchInfoOf,
		TransactionExtension, ValidateResult, Zero,
	},
	transaction_validity::{
		InvalidTransaction, TransactionLongevity, TransactionValidityError, ValidTransaction,
	},
	DispatchResult, Saturating,
};
use sp_weights::Weight;

/// Nonce check and increment to give replay protection for transactions.
///
/// # Transaction Validity
///
/// This extension affects `requires` and `provides` tags of validity, but DOES NOT
/// set the `priority` field. Make sure that AT LEAST one of the transaction extension sets
/// some kind of priority upon validating transactions.
///
/// The preparation step assumes that the nonce information has not changed since the validation
/// step. This means that other extensions ahead of `CheckNonce` in the pipeline must not alter the
/// nonce during their own preparation step, or else the transaction may be rejected during dispatch
/// or lead to an inconsistent account state.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq)]
pub struct CheckNonce<T: Config>(#[codec(compact)] pub T::Nonce);

/// For a valid transaction the provides and requires information related to the nonce.
pub struct ValidNonceInfo {
	/// The encoded `provides` used for this transaction.
	pub provides: Vec<Vec<u8>>,
	/// The encoded `requires` used for this transaction.
	pub requires: Vec<Vec<u8>>,
}

impl<T: Config> CheckNonce<T> {
	/// utility constructor. Used only in client/factory code.
	pub fn from(nonce: T::Nonce) -> Self {
		Self(nonce)
	}

	/// In transaction extension, validate nonce for account, on success returns provides and
	/// requires.
	pub fn validate_nonce_for_account(
		who: &T::AccountId,
		nonce: T::Nonce,
	) -> Result<ValidNonceInfo, TransactionValidityError> {
		let account = crate::Account::<T>::get(who);
		if account.providers.is_zero() && account.sufficients.is_zero() {
			// Nonce storage not paid for
			return Err(InvalidTransaction::Payment.into())
		}
		if nonce < account.nonce {
			return Err(InvalidTransaction::Stale.into())
		}

		let provides = vec![Encode::encode(&(who.clone(), nonce))];
		let requires = if account.nonce < nonce {
			vec![Encode::encode(&(who.clone(), nonce.saturating_sub(One::one())))]
		} else {
			vec![]
		};

		Ok(ValidNonceInfo { provides, requires })
	}

	/// In transaction extension, prepare nonce for account.
	pub fn prepare_nonce_for_account(
		who: &T::AccountId,
		mut nonce: T::Nonce,
	) -> Result<(), TransactionValidityError> {
		let account = crate::Account::<T>::get(who);
		if nonce > account.nonce {
			return Err(InvalidTransaction::Future.into())
		}
		nonce = nonce.checked_add(&T::Nonce::one()).unwrap_or(T::Nonce::zero());
		crate::Account::<T>::mutate(who, |account| account.nonce = nonce);
		Ok(())
	}
}

impl<T: Config> core::fmt::Debug for CheckNonce<T> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		write!(f, "CheckNonce({})", self.0)
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
		Ok(())
	}
}

/// Operation to perform from `validate` to `prepare` in [`CheckNonce`] transaction extension.
#[derive(RuntimeDebugNoBound)]
pub enum Val<T: Config> {
	/// Account and its nonce to check for.
	CheckNonce(T::AccountId),
	/// Weight to refund.
	Refund(Weight),
}

/// Operation to perform from `prepare` to `post_dispatch_details` in [`CheckNonce`] transaction
/// extension.
#[derive(RuntimeDebugNoBound)]
pub enum Pre {
	/// The transaction extension weight should not be refunded.
	NonceChecked,
	/// The transaction extension weight should be refunded.
	Refund(Weight),
}

impl<T: Config> TransactionExtension<T::RuntimeCall> for CheckNonce<T>
where
	T::RuntimeCall: Dispatchable<Info = DispatchInfo>,
	<T::RuntimeCall as Dispatchable>::RuntimeOrigin: AsSystemOriginSigner<T::AccountId> + Clone,
{
	const IDENTIFIER: &'static str = "CheckNonce";
	type Implicit = ();
	type Val = Val<T>;
	type Pre = Pre;

	fn weight(&self, _: &T::RuntimeCall) -> sp_weights::Weight {
		<T::ExtensionsWeightInfo as super::WeightInfo>::check_nonce()
	}

	fn validate(
		&self,
		origin: <T as Config>::RuntimeOrigin,
		call: &T::RuntimeCall,
		_info: &DispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_self_implicit: Self::Implicit,
		_inherited_implication: &impl Encode,
		_source: TransactionSource,
	) -> ValidateResult<Self::Val, T::RuntimeCall> {
		let Some(who) = origin.as_system_origin_signer() else {
			return Ok((Default::default(), Val::Refund(self.weight(call)), origin))
		};
		let ValidNonceInfo { provides, requires } = Self::validate_nonce_for_account(who, self.0)?;

		let validity = ValidTransaction {
			priority: 0,
			requires,
			provides,
			longevity: TransactionLongevity::max_value(),
			propagate: true,
		};

		Ok((validity, Val::CheckNonce(who.clone()), origin))
	}

	fn prepare(
		self,
		val: Self::Val,
		_origin: &T::RuntimeOrigin,
		_call: &T::RuntimeCall,
		_info: &DispatchInfoOf<T::RuntimeCall>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		let (who, nonce) = match val {
			Val::CheckNonce(who) => (who, self.0),
			Val::Refund(weight) => return Ok(Pre::Refund(weight)),
		};
		Self::prepare_nonce_for_account(&who, nonce).map(|_| Pre::NonceChecked)
	}

	fn post_dispatch_details(
		pre: Self::Pre,
		_info: &DispatchInfo,
		_post_info: &PostDispatchInfoOf<T::RuntimeCall>,
		_len: usize,
		_result: &DispatchResult,
	) -> Result<Weight, TransactionValidityError> {
		match pre {
			Pre::NonceChecked => Ok(Weight::zero()),
			Pre::Refund(weight) => Ok(weight),
		}
	}
}

#[cfg(test)]
mod tests {
}
