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

//! Generic implementation of an unchecked (pre-verification) extrinsic.

use crate::{
	generic::{CheckedExtrinsic, ExtrinsicFormat},
	traits::{
		self, transaction_extension::TransactionExtension, Checkable, Dispatchable, ExtrinsicCall,
		ExtrinsicLike, /* ExtrinsicMetadata,*/ IdentifyAccount, MaybeDisplay, Member, SignaturePayload,
	},
	transaction_validity::{InvalidTransaction, TransactionValidityError},
	OpaqueExtrinsic,
};
#[cfg(all(not(feature = "std"), feature = "serde"))]
use alloc::format;
use alloc::vec::Vec;
use codec::{
	Compact, CountedInput, Decode, DecodeWithMemLimit, DecodeWithMemTracking, Encode, EncodeLike,
	Input,
};
use core::fmt;
use sp_io::hashing::blake2_256;
use sp_weights::Weight;

/// Type to represent the version of the [Extension](TransactionExtension) used in this extrinsic.
pub type ExtensionVersion = u8;
/// Type to represent the extrinsic format version which defines an [UncheckedExtrinsic].
pub type ExtrinsicVersion = u8;

/// Current version of the [`UncheckedExtrinsic`] encoded format.
///
/// This version needs to be bumped if the encoded representation changes.
/// It ensures that if the representation is changed and the format is not known,
/// the decoding fails.
pub const EXTRINSIC_FORMAT_VERSION: ExtrinsicVersion = 5;
/// Legacy version of the [`UncheckedExtrinsic`] encoded format.
///
/// This version was used in the signed/unsigned transaction model and is still supported for
/// compatibility reasons. It will be deprecated in favor of v5 extrinsics and an inherent/general
/// transaction model.
pub const LEGACY_EXTRINSIC_FORMAT_VERSION: ExtrinsicVersion = 4;
/// Current version of the [Extension](TransactionExtension) used in this
/// [extrinsic](UncheckedExtrinsic).
///
/// This version needs to be bumped if there are breaking changes to the extension used in the
/// [UncheckedExtrinsic] implementation.
const EXTENSION_VERSION: ExtensionVersion = 0;

/// Maximum decoded heap size for a runtime call (in bytes).
pub const DEFAULT_MAX_CALL_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

/// The `SignaturePayload` of `UncheckedExtrinsic`.
pub type UncheckedSignaturePayload<Address, Signature, Extension> = (Address, Signature, Extension);

impl<Address, Signature, Extension> SignaturePayload
	for UncheckedSignaturePayload<Address, Signature, Extension>
{
	type SignatureAddress = Address;
	type Signature = Signature;
	type SignatureExtra = Extension;
}

/// A "header" for extrinsics leading up to the call itself. Determines the type of extrinsic and
/// holds any necessary specialized data.
#[derive(DecodeWithMemTracking, Eq, PartialEq, Clone)]
pub enum Preamble<Address, Signature, Extension> {
	/// An extrinsic without a signature or any extension. This means it's either an inherent or
	/// an old-school "Unsigned" (we don't use that terminology any more since it's confusable with
	/// the general transaction which is without a signature but does have an extension).
	///
	/// NOTE: In the future, once we remove `ValidateUnsigned`, this will only serve Inherent
	/// extrinsics and thus can be renamed to `Inherent`.
	Bare(ExtrinsicVersion),
	/// An old-school transaction extrinsic which includes a signature of some hard-coded crypto.
	/// Available only on extrinsic version 4.
	Signed(Address, Signature, Extension),
	/// A new-school transaction extrinsic which does not include a signature by default. The
	/// origin authorization, through signatures or other means, is performed by the transaction
	/// extension in this extrinsic. Available starting with extrinsic version 5.
	General(ExtensionVersion, Extension),
}

const VERSION_MASK: u8 = 0b0011_1111;
const TYPE_MASK: u8 = 0b1100_0000;
const BARE_EXTRINSIC: u8 = 0b0000_0000;
const SIGNED_EXTRINSIC: u8 = 0b1000_0000;
const GENERAL_EXTRINSIC: u8 = 0b0100_0000;

impl<Address, Signature, Extension> Decode for Preamble<Address, Signature, Extension>
where
	Address: Decode,
	Signature: Decode,
	Extension: Decode,
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
		let version_and_type = input.read_byte()?;

		let version = version_and_type & VERSION_MASK;
		let xt_type = version_and_type & TYPE_MASK;

		let preamble = match (version, xt_type) {
			(
				extrinsic_version @ LEGACY_EXTRINSIC_FORMAT_VERSION..=EXTRINSIC_FORMAT_VERSION,
				BARE_EXTRINSIC,
			) => Self::Bare(extrinsic_version),
			(LEGACY_EXTRINSIC_FORMAT_VERSION, SIGNED_EXTRINSIC) => {
				let address = Address::decode(input)?;
				let signature = Signature::decode(input)?;
				let ext = Extension::decode(input)?;
				Self::Signed(address, signature, ext)
			},
			(EXTRINSIC_FORMAT_VERSION, GENERAL_EXTRINSIC) => {
				let ext_version = ExtensionVersion::decode(input)?;
				let ext = Extension::decode(input)?;
				Self::General(ext_version, ext)
			},
			(_, _) => return Err("Invalid transaction version".into()),
		};

		Ok(preamble)
	}
}

impl<Address, Signature, Extension> Encode for Preamble<Address, Signature, Extension>
where
	Address: Encode,
	Signature: Encode,
	Extension: Encode,
{
	fn size_hint(&self) -> usize {
		match &self {
			Preamble::Bare(_) => EXTRINSIC_FORMAT_VERSION.size_hint(),
			Preamble::Signed(address, signature, ext) => LEGACY_EXTRINSIC_FORMAT_VERSION
				.size_hint()
				.saturating_add(address.size_hint())
				.saturating_add(signature.size_hint())
				.saturating_add(ext.size_hint()),
			Preamble::General(ext_version, ext) => EXTRINSIC_FORMAT_VERSION
				.size_hint()
				.saturating_add(ext_version.size_hint())
				.saturating_add(ext.size_hint()),
		}
	}

	fn encode_to<T: codec::Output + ?Sized>(&self, dest: &mut T) {
		match &self {
			Preamble::Bare(extrinsic_version) => {
				(extrinsic_version | BARE_EXTRINSIC).encode_to(dest);
			},
			Preamble::Signed(address, signature, ext) => {
				(LEGACY_EXTRINSIC_FORMAT_VERSION | SIGNED_EXTRINSIC).encode_to(dest);
				address.encode_to(dest);
				signature.encode_to(dest);
				ext.encode_to(dest);
			},
			Preamble::General(ext_version, ext) => {
				(EXTRINSIC_FORMAT_VERSION | GENERAL_EXTRINSIC).encode_to(dest);
				ext_version.encode_to(dest);
				ext.encode_to(dest);
			},
		}
	}
}

impl<Address, Signature, Extension> Preamble<Address, Signature, Extension> {
	/// Returns `Some` if this is a signed extrinsic, together with the relevant inner fields.
	pub fn to_signed(self) -> Option<(Address, Signature, Extension)> {
		match self {
			Self::Signed(a, s, e) => Some((a, s, e)),
			_ => None,
		}
	}
}

impl<Address, Signature, Extension> fmt::Debug for Preamble<Address, Signature, Extension>
where
	Address: fmt::Debug,
	Extension: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::Bare(_) => write!(f, "Bare"),
			Self::Signed(address, _, tx_ext) => write!(f, "Signed({:?}, {:?})", address, tx_ext),
			Self::General(ext_version, tx_ext) =>
				write!(f, "General({:?}, {:?})", ext_version, tx_ext),
		}
	}
}

/// An extrinsic right from the external world. This is unchecked and so can contain a signature.
///
/// An extrinsic is formally described as any external data that is originating from the outside of
/// the runtime and fed into the runtime as a part of the block-body.
///
/// Inherents are special types of extrinsics that are placed into the block by the block-builder.
/// They are unsigned because the assertion is that they are "inherently true" by virtue of getting
/// past all validators.
///
/// Transactions are all other statements provided by external entities that the chain deems values
/// and decided to include in the block. This value is typically in the form of fee payment, but it
/// could in principle be any other interaction. Transactions are either signed or unsigned. A
/// sensible transaction pool should ensure that only transactions that are worthwhile are
/// considered for block-building.
/// This type is by no means enforced within Substrate, but given its genericness, it is highly
/// likely that for most use-cases it will suffice. Thus, the encoding of this type will dictate
/// exactly what bytes should be sent to a runtime to transact with it.
///
/// This can be checked using [`Checkable`], yielding a [`CheckedExtrinsic`], which is the
/// counterpart of this type after its signature (and other non-negotiable validity checks) have
/// passed.
#[derive(DecodeWithMemTracking, PartialEq, Eq, Clone, Debug)]
#[codec(decode_with_mem_tracking_bound(
	Address: DecodeWithMemTracking,
	Call: DecodeWithMemTracking,
	Signature: DecodeWithMemTracking,
	Extension: DecodeWithMemTracking)
)]
pub struct UncheckedExtrinsic<
	Address,
	Call,
	Signature,
	Extension,
	const MAX_CALL_SIZE: usize = DEFAULT_MAX_CALL_SIZE,
> {
	/// Information regarding the type of extrinsic this is (inherent or transaction) as well as
	/// associated extension (`Extension`) data if it's a transaction and a possible signature.
	pub preamble: Preamble<Address, Signature, Extension>,
	/// The function that should be called.
	pub function: Call,
}

impl<Address, Call, Signature, Extension> UncheckedExtrinsic<Address, Call, Signature, Extension> {
	/// New instance of a bare (ne unsigned) extrinsic. This could be used for an inherent or an
	/// old-school "unsigned transaction" (which are new being deprecated in favour of general
	/// transactions).
	#[deprecated = "Use new_bare instead"]
	pub fn new_unsigned(function: Call) -> Self {
		Self::new_bare(function)
	}

	/// Returns `true` if this extrinsic instance is an inherent, `false`` otherwise.
	pub fn is_inherent(&self) -> bool {
		matches!(self.preamble, Preamble::Bare(_))
	}

	/// Returns `true` if this extrinsic instance is an old-school signed transaction, `false`
	/// otherwise.
	pub fn is_signed(&self) -> bool {
		matches!(self.preamble, Preamble::Signed(..))
	}

	/// Create an `UncheckedExtrinsic` from a `Preamble` and the actual `Call`.
	pub fn from_parts(function: Call, preamble: Preamble<Address, Signature, Extension>) -> Self {
		Self { preamble, function }
	}

	/// New instance of a bare (ne unsigned) extrinsic.
	pub fn new_bare(function: Call) -> Self {
		Self::from_parts(function, Preamble::Bare(EXTRINSIC_FORMAT_VERSION))
	}

	/// New instance of a bare (ne unsigned) extrinsic on extrinsic format version 4.
	pub fn new_bare_legacy(function: Call) -> Self {
		Self::from_parts(function, Preamble::Bare(LEGACY_EXTRINSIC_FORMAT_VERSION))
	}

	/// New instance of an old-school signed transaction on extrinsic format version 4.
	pub fn new_signed(
		function: Call,
		signed: Address,
		signature: Signature,
		tx_ext: Extension,
	) -> Self {
		Self::from_parts(function, Preamble::Signed(signed, signature, tx_ext))
	}

	/// New instance of an new-school unsigned transaction.
	pub fn new_transaction(function: Call, tx_ext: Extension) -> Self {
		Self::from_parts(function, Preamble::General(EXTENSION_VERSION, tx_ext))
	}
}

impl<Address, Call, Signature, Extension> ExtrinsicLike
	for UncheckedExtrinsic<Address, Call, Signature, Extension>
{
	fn is_signed(&self) -> Option<bool> {
		Some(matches!(self.preamble, Preamble::Signed(..)))
	}

	fn is_bare(&self) -> bool {
		matches!(self.preamble, Preamble::Bare(_))
	}
}

impl<Address, Call, Signature, Extra> ExtrinsicCall
	for UncheckedExtrinsic<Address, Call, Signature, Extra>
{
	type Call = Call;

	fn call(&self) -> &Call {
		&self.function
	}
}

// TODO: Migrate existing extension pipelines to support current `Signed` transactions as `General`
// transactions by adding an extension to validate signatures, as they are currently validated in
// the `Checkable` implementation for `Signed` transactions.

impl<LookupSource, AccountId, Call, Signature, Extension, Lookup> Checkable<Lookup>
	for UncheckedExtrinsic<LookupSource, Call, Signature, Extension>
where
	LookupSource: Member + MaybeDisplay,
	Call: Encode + Member + Dispatchable,
	Signature: Member + traits::Verify,
	<Signature as traits::Verify>::Signer: IdentifyAccount<AccountId = AccountId>,
	Extension: Encode + TransactionExtension<Call>,
	AccountId: Member + MaybeDisplay,
	Lookup: traits::Lookup<Source = LookupSource, Target = AccountId>,
{
	type Checked = CheckedExtrinsic<AccountId, Call, Extension>;

	fn check(self, lookup: &Lookup) -> Result<Self::Checked, TransactionValidityError> {
		Ok(match self.preamble {
			Preamble::Signed(signed, signature, tx_ext) => {
				let signed = lookup.lookup(signed)?;
				// The `Implicit` is "implicitly" included in the payload.
				let raw_payload = SignedPayload::new(self.function, tx_ext)?;
				if !raw_payload.using_encoded(|payload| signature.verify(payload, &signed)) {
					return Err(InvalidTransaction::BadProof.into())
				}
				let (function, tx_ext, _) = raw_payload.deconstruct();
				CheckedExtrinsic { format: ExtrinsicFormat::Signed(signed, tx_ext), function }
			},
			Preamble::General(extension_version, tx_ext) => CheckedExtrinsic {
				format: ExtrinsicFormat::General(extension_version, tx_ext),
				function: self.function,
			},
			Preamble::Bare(_) =>
				CheckedExtrinsic { format: ExtrinsicFormat::Bare, function: self.function },
		})
	}

	#[cfg(feature = "try-runtime")]
	fn unchecked_into_checked_i_know_what_i_am_doing(
		self,
		lookup: &Lookup,
	) -> Result<Self::Checked, TransactionValidityError> {
		Ok(match self.preamble {
			Preamble::Signed(signed, _, tx_ext) => {
				let signed = lookup.lookup(signed)?;
				CheckedExtrinsic {
					format: ExtrinsicFormat::Signed(signed, tx_ext),
					function: self.function,
				}
			},
			Preamble::General(extension_version, tx_ext) => CheckedExtrinsic {
				format: ExtrinsicFormat::General(extension_version, tx_ext),
				function: self.function,
			},
			Preamble::Bare(_) =>
				CheckedExtrinsic { format: ExtrinsicFormat::Bare, function: self.function },
		})
	}
}

/* impl<Address, Call: Dispatchable, Signature, Extension: TransactionExtension<Call>>
	ExtrinsicMetadata for UncheckedExtrinsic<Address, Call, Signature, Extension>
{
	const VERSIONS: &'static [u8] = &[LEGACY_EXTRINSIC_FORMAT_VERSION, EXTRINSIC_FORMAT_VERSION];
	type TransactionExtensions = Extension;
} */

impl<Address, Call: Dispatchable, Signature, Extension: TransactionExtension<Call>>
	UncheckedExtrinsic<Address, Call, Signature, Extension>
{
	/// Returns the weight of the extension of this transaction, if present. If the transaction
	/// doesn't use any extension, the weight returned is equal to zero.
	pub fn extension_weight(&self) -> Weight {
		match &self.preamble {
			Preamble::Bare(_) => Weight::zero(),
			Preamble::Signed(_, _, ext) | Preamble::General(_, ext) => ext.weight(&self.function),
		}
	}
}

impl<Address, Call, Signature, Extension, const MAX_CALL_SIZE: usize> Decode
	for UncheckedExtrinsic<Address, Call, Signature, Extension, MAX_CALL_SIZE>
where
	Address: Decode,
	Signature: Decode,
	Call: DecodeWithMemTracking,
	Extension: Decode,
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
		// This is a little more complicated than usual since the binary format must be compatible
		// with SCALE's generic `Vec<u8>` type. Basically this just means accepting that there
		// will be a prefix of vector length.
		let expected_length: Compact<u32> = Decode::decode(input)?;
		let mut input = CountedInput::new(input);

		let preamble = Decode::decode(&mut input)?;
		// Adds 1 byte to the `MAX_CALL_SIZE` as the decoding fails exactly at the given value and
		// the maximum should be allowed to fit in.
		let function = Call::decode_with_mem_limit(&mut input, MAX_CALL_SIZE.saturating_add(1))?;

		if input.count() != expected_length.0 as u64 {
			return Err("Invalid length prefix".into())
		}

		Ok(Self { preamble, function })
	}
}

impl<Address, Call, Signature, Extension> Encode
	for UncheckedExtrinsic<Address, Call, Signature, Extension>
where
	Preamble<Address, Signature, Extension>: Encode,
	Call: Encode,
	Extension: Encode,
{
	fn encode(&self) -> Vec<u8> {
		let mut tmp = self.preamble.encode();
		self.function.encode_to(&mut tmp);

		let compact_len = codec::Compact::<u32>(tmp.len() as u32);

		// Allocate the output buffer with the correct length
		let mut output = Vec::with_capacity(compact_len.size_hint() + tmp.len());

		compact_len.encode_to(&mut output);
		output.extend(tmp);

		output
	}
}

impl<Address, Call, Signature, Extension> EncodeLike
	for UncheckedExtrinsic<Address, Call, Signature, Extension>
where
	Address: Encode,
	Signature: Encode,
	Call: Encode + Dispatchable,
	Extension: TransactionExtension<Call>,
{
}

#[cfg(feature = "serde")]
impl<Address: Encode, Signature: Encode, Call: Encode, Extension: Encode> serde::Serialize
	for UncheckedExtrinsic<Address, Call, Signature, Extension>
{
	fn serialize<S>(&self, seq: S) -> Result<S::Ok, S::Error>
	where
		S: ::serde::Serializer,
	{
		self.using_encoded(|bytes| seq.serialize_bytes(bytes))
	}
}

#[cfg(feature = "serde")]
impl<'a, Address: Decode, Signature: Decode, Call: DecodeWithMemTracking, Extension: Decode>
	serde::Deserialize<'a> for UncheckedExtrinsic<Address, Call, Signature, Extension>
{
	fn deserialize<D>(de: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'a>,
	{
		let r = sp_core::bytes::deserialize(de)?;
		Self::decode(&mut &r[..])
			.map_err(|e| serde::de::Error::custom(format!("Decode error: {}", e)))
	}
}

/// A payload that has been signed for an unchecked extrinsics.
///
/// Note that the payload that we sign to produce unchecked extrinsic signature
/// is going to be different than the `SignaturePayload` - so the thing the extrinsic
/// actually contains.
pub struct SignedPayload<Call: Dispatchable, Extension: TransactionExtension<Call>>(
	(Call, Extension, Extension::Implicit),
);

impl<Call, Extension> SignedPayload<Call, Extension>
where
	Call: Encode + Dispatchable,
	Extension: TransactionExtension<Call>,
{
	/// Create new `SignedPayload` for extrinsic format version 4.
	///
	/// This function may fail if `implicit` of `Extension` is not available.
	pub fn new(call: Call, tx_ext: Extension) -> Result<Self, TransactionValidityError> {
		let implicit = Extension::implicit(&tx_ext)?;
		let raw_payload = (call, tx_ext, implicit);
		Ok(Self(raw_payload))
	}

	/// Create new `SignedPayload` from raw components.
	pub fn from_raw(call: Call, tx_ext: Extension, implicit: Extension::Implicit) -> Self {
		Self((call, tx_ext, implicit))
	}

	/// Deconstruct the payload into it's components.
	pub fn deconstruct(self) -> (Call, Extension, Extension::Implicit) {
		self.0
	}
}

impl<Call, Extension> Encode for SignedPayload<Call, Extension>
where
	Call: Encode + Dispatchable,
	Extension: TransactionExtension<Call>,
{
	/// Get an encoded version of this `blake2_256`-hashed payload.
	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		self.0.using_encoded(|payload| {
			if payload.len() > 256 {
				f(&blake2_256(payload)[..])
			} else {
				f(payload)
			}
		})
	}
}

impl<Call, Extension> EncodeLike for SignedPayload<Call, Extension>
where
	Call: Encode + Dispatchable,
	Extension: TransactionExtension<Call>,
{
}

impl<Address, Call, Signature, Extension>
	From<UncheckedExtrinsic<Address, Call, Signature, Extension>> for OpaqueExtrinsic
where
	Address: Encode,
	Signature: Encode,
	Call: Encode,
	Extension: Encode,
{
	fn from(extrinsic: UncheckedExtrinsic<Address, Call, Signature, Extension>) -> Self {
		Self::from_bytes(extrinsic.encode().as_slice()).expect(
			"both OpaqueExtrinsic and UncheckedExtrinsic have encoding that is compatible with \
				raw Vec<u8> encoding",
		)
	}
}

#[cfg(test)]
mod legacy {
	use codec::{Compact, Decode, Encode, EncodeLike, Error, Input};
	use scale_info::{
		build::Fields, meta_type, Path, Type, TypeParameter,
	};

	pub type UncheckedSignaturePayloadV4<Address, Signature, Extra> = (Address, Signature, Extra);

	#[derive(PartialEq, Eq, Clone, Debug)]
	pub struct UncheckedExtrinsicV4<Address, Call, Signature, Extra> {
		pub signature: Option<UncheckedSignaturePayloadV4<Address, Signature, Extra>>,
		pub function: Call,
	}

	impl<Address, Call, Signature, Extra> UncheckedExtrinsicV4<Address, Call, Signature, Extra> {
		pub fn new_signed(
			function: Call,
			signed: Address,
			signature: Signature,
			extra: Extra,
		) -> Self {
			Self { signature: Some((signed, signature, extra)), function }
		}

		pub fn new_unsigned(function: Call) -> Self {
			Self { signature: None, function }
		}
	}

	impl<Address, Call, Signature, Extra> Decode
		for UncheckedExtrinsicV4<Address, Call, Signature, Extra>
	where
		Address: Decode,
		Signature: Decode,
		Call: Decode,
		Extra: Decode,
	{
		fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
			// This is a little more complicated than usual since the binary format must be
			// compatible with SCALE's generic `Vec<u8>` type. Basically this just means accepting
			// that there will be a prefix of vector length.
			let expected_length: Compact<u32> = Decode::decode(input)?;
			let before_length = input.remaining_len()?;

			let version = input.read_byte()?;

			let is_signed = version & 0b1000_0000 != 0;
			let version = version & 0b0111_1111;
			if version != 4u8 {
				return Err("Invalid transaction version".into())
			}

			let signature = is_signed.then(|| Decode::decode(input)).transpose()?;
			let function = Decode::decode(input)?;

			if let Some((before_length, after_length)) =
				input.remaining_len()?.and_then(|a| before_length.map(|b| (b, a)))
			{
				let length = before_length.saturating_sub(after_length);

				if length != expected_length.0 as usize {
					return Err("Invalid length prefix".into())
				}
			}

			Ok(Self { signature, function })
		}
	}

	impl<Address, Call, Signature, Extra> Encode
		for UncheckedExtrinsicV4<Address, Call, Signature, Extra>
	where
		Address: Encode,
		Signature: Encode,
		Call: Encode,
		Extra: Encode,
	{
		fn encode(&self) -> Vec<u8> {
			let mut tmp = Vec::with_capacity(core::mem::size_of::<Self>());

			// 1 byte version id.
			match self.signature.as_ref() {
				Some(s) => {
					tmp.push(4u8 | 0b1000_0000);
					s.encode_to(&mut tmp);
				},
				None => {
					tmp.push(4u8 & 0b0111_1111);
				},
			}
			self.function.encode_to(&mut tmp);

			let compact_len = codec::Compact::<u32>(tmp.len() as u32);

			// Allocate the output buffer with the correct length
			let mut output = Vec::with_capacity(compact_len.size_hint() + tmp.len());

			compact_len.encode_to(&mut output);
			output.extend(tmp);

			output
		}
	}

	impl<Address, Call, Signature, Extra> EncodeLike
		for UncheckedExtrinsicV4<Address, Call, Signature, Extra>
	where
		Address: Encode,
		Signature: Encode,
		Call: Encode,
		Extra: Encode,
	{
	}
}

#[cfg(test)]
mod tests {
}
