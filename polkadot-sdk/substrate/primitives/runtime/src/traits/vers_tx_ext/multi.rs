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

//! Types and trait to aggregate multiple versioned transaction extension pipelines.

use crate::{
	traits::{
		DecodeWithVersion, DecodeWithVersionWithMemTracking, DispatchInfoOf, DispatchOriginOf,
		Dispatchable, InvalidVersion, Pipeline, PipelineAtVers, /* PipelineMetadataBuilder, */
		PipelineVersion, PostDispatchInfoOf,
	},
	transaction_validity::{TransactionSource, TransactionValidityError, ValidTransaction},
};
use alloc::vec::Vec;
use codec::Encode;
use core::fmt::Debug;
use sp_weights::Weight;

/// An item in [`MultiVersion`]. It represents a transaction extension pipeline of a specific
/// single version.
pub trait MultiVersionItem {
	/// The version of the transaction extension pipeline.
	///
	/// `None` means that the item has no version and can't be decoded.
	const VERSION: Option<u8>;
}

impl MultiVersionItem for InvalidVersion {
	const VERSION: Option<u8> = None;
}

impl<const VERSION: u8, Extension> MultiVersionItem for PipelineAtVers<VERSION, Extension> {
	const VERSION: Option<u8> = Some(VERSION);
}

macro_rules! declare_multi_version_enum {
	($( $variant:tt, )*) => {

		/// An implementation of [`Pipeline`] that aggregates multiple versioned transaction
		/// extension pipeline.
		///
		/// It is an enum where each variant has its own version, duplicated version must be
		/// avoided, only the first used version will be effective other duplicated version will be
		/// ignored.
		///
		/// Versioned transaction extension pipelines are configured using the generic parameters.
		///
		/// # Example
		///
		/// ```
		/// use sp_runtime::traits::{MultiVersion, PipelineAtVers};
		///
		/// struct PaymentExt;
		/// struct PaymentExtV2;
		/// struct NonceExt;
		///
		/// type ExtV1 = PipelineAtVers<1, (NonceExt, PaymentExt)>;
		/// type ExtV4 = PipelineAtVers<4, (NonceExt, PaymentExtV2)>;
		///
		/// /// The transaction extension pipeline that supports both version 1 and 4.
		/// type TransactionExtension = MultiVersion<ExtV1, ExtV4>;
		/// ```
		#[allow(private_interfaces)]
		#[derive(PartialEq, Eq, Clone, Debug)]
		pub enum MultiVersion<
			$(
				$variant = InvalidVersion,
			)*
		> {
			$(
				/// The transaction extension pipeline of a specific version.
				$variant($variant),
			)*
		}

		impl<$( $variant: PipelineVersion, )*> PipelineVersion for MultiVersion<$( $variant, )*> {
			fn version(&self) -> u8 {
				match self {
					$(
						MultiVersion::$variant(v) => v.version(),
					)*
				}
			}
		}

		// It encodes without the variant index.
		impl<$( $variant: Encode, )*> Encode for MultiVersion<$( $variant, )*> {
			fn size_hint(&self) -> usize {
				match self {
					$(
						MultiVersion::$variant(v) => v.size_hint(),
					)*
				}
			}
			fn encode(&self) -> Vec<u8> {
				match self {
					$(
						MultiVersion::$variant(v) => v.encode(),
					)*
				}
			}
			fn encode_to<CodecOutput: codec::Output + ?Sized>(&self, dest: &mut CodecOutput) {
				match self {
					$(
						MultiVersion::$variant(v) => v.encode_to(dest),
					)*
				}
			}
			fn encoded_size(&self) -> usize {
				match self {
					$(
						MultiVersion::$variant(v) => v.encoded_size(),
					)*
				}
			}
			fn using_encoded<FunctionResult, Function: FnOnce(&[u8]) -> FunctionResult>(
				&self,
				f: Function
			) -> FunctionResult {
				match self {
					$(
						MultiVersion::$variant(v) => v.using_encoded(f),
					)*
				}
			}
		}

		// It decodes from a specified version.
		impl<$( $variant: DecodeWithVersion + MultiVersionItem, )*>
			DecodeWithVersion for MultiVersion<$( $variant, )*>
		{
			fn decode_with_version<CodecInput: codec::Input>(
				extension_version: u8,
				input: &mut CodecInput,
			) -> Result<Self, codec::Error> {
				$(
					// Here we could try all variants without checking for the version,
					// but the error would be less informative.
					// Otherwise we could change the trait `DecodeWithVersion` to return an enum of
					// 3 variants: ok, error and invalid_version.
					if $variant::VERSION == Some(extension_version) {
						return Ok(MultiVersion::$variant($variant::decode_with_version(extension_version, input)?));
					}
				)*

				Err(codec::Error::from("Invalid extension version"))
			}
		}

		impl<$( $variant: DecodeWithVersionWithMemTracking + MultiVersionItem, )*>
			DecodeWithVersionWithMemTracking for MultiVersion<$( $variant, )*>
		{}

		impl<$( $variant: Pipeline<Call> + MultiVersionItem, )* Call: Dispatchable>
			Pipeline<Call> for MultiVersion<$( $variant, )*>
		{
			/* fn build_metadata(builder: &mut PipelineMetadataBuilder) {
				$(
					$variant::build_metadata(builder);
				)*
			} */
			fn validate_only(
				&self,
				origin: DispatchOriginOf<Call>,
				call: &Call,
				info: &DispatchInfoOf<Call>,
				len: usize,
				source: TransactionSource,
			) -> Result<ValidTransaction, TransactionValidityError> {
				match self {
					$(
						MultiVersion::$variant(v) => v.validate_only(origin, call, info, len, source),
					)*
				}
			}
			fn dispatch_transaction(
				self,
				origin: DispatchOriginOf<Call>,
				call: Call,
				info: &DispatchInfoOf<Call>,
				len: usize,
			) -> crate::ApplyExtrinsicResultWithInfo<PostDispatchInfoOf<Call>> {
				match self {
					$(
						MultiVersion::$variant(v) => v.dispatch_transaction(origin, call, info, len),
					)*
				}
			}
			fn weight(&self, call: &Call) -> Weight {
				match self {
					$(
						MultiVersion::$variant(v) => v.weight(call),
					)*
				}
			}
		}
	};
}

declare_multi_version_enum! {
	A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
}

#[cfg(test)]
mod tests {
}
