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

//! Simple sr25519 (Schnorr-Ristretto) API.
//!
//! Note: `CHAIN_CODE_LENGTH` must be equal to `crate::crypto::JUNCTION_ID_LEN`
//! for this to work.

#[cfg(feature = "serde")]
use crate::crypto::Ss58Codec;
use crate::{
	crypto::{CryptoBytes, DeriveError, DeriveJunction, Pair as TraitPair, SecretStringError},
	proof_of_possession::NonAggregatable,
};

use alloc::vec::Vec;
#[cfg(feature = "full_crypto")]
use schnorrkel::signing_context;
use schnorrkel::{
	derive::{ChainCode, Derivation, CHAIN_CODE_LENGTH},
	ExpansionMode, Keypair, MiniSecretKey, PublicKey, SecretKey,
};

use crate::crypto::{CryptoType, CryptoTypeId, Derive, Public as TraitPublic, SignatureBytes};
use codec::{Decode, Encode, MaxEncodedLen};

#[cfg(all(not(feature = "std"), feature = "serde"))]
use alloc::{format, string::String};
use schnorrkel::keys::{MINI_SECRET_KEY_LENGTH, SECRET_KEY_LENGTH};
#[cfg(feature = "serde")]
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

// signing context
const SIGNING_CTX: &[u8] = b"substrate";

/// An identifier used to match public keys against sr25519 keys
pub const CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"sr25");

/// The byte length of public key
pub const PUBLIC_KEY_SERIALIZED_SIZE: usize = 32;

/// The byte length of signature
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;

#[doc(hidden)]
pub struct Sr25519Tag;
#[doc(hidden)]
pub struct Sr25519PublicTag;

/// An Schnorrkel/Ristretto x25519 ("sr25519") public key.
pub type Public = CryptoBytes<PUBLIC_KEY_SERIALIZED_SIZE, Sr25519PublicTag>;

impl TraitPublic for Public {}

impl Derive for Public {
	/// Derive a child key from a series of given junctions.
	///
	/// `None` if there are any hard junctions in there.
	#[cfg(feature = "serde")]
	fn derive<Iter: Iterator<Item = DeriveJunction>>(&self, path: Iter) -> Option<Public> {
		let mut acc = PublicKey::from_bytes(self.as_ref()).ok()?;
		for j in path {
			match j {
				DeriveJunction::Soft(cc) => acc = acc.derived_key_simple(ChainCode(cc), &[]).0,
				DeriveJunction::Hard(_cc) => return None,
			}
		}
		Some(Self::from(acc.to_bytes()))
	}
}

#[cfg(feature = "std")]
impl std::str::FromStr for Public {
	type Err = crate::crypto::PublicError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Self::from_ss58check(s)
	}
}

#[cfg(feature = "std")]
impl std::fmt::Display for Public {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "{}", self.to_ss58check())
	}
}

impl core::fmt::Debug for Public {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		let s = self.to_ss58check();
		write!(f, "{} ({}...)", crate::hexdisplay::HexDisplay::from(&self.0), &s[0..8])
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
		Ok(())
	}
}

#[cfg(feature = "serde")]
impl Serialize for Public {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(&self.to_ss58check())
	}
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Public {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Public::from_ss58check(&String::deserialize(deserializer)?)
			.map_err(|e| de::Error::custom(format!("{:?}", e)))
	}
}

/// An Schnorrkel/Ristretto x25519 ("sr25519") signature.
pub type Signature = SignatureBytes<SIGNATURE_SERIALIZED_SIZE, Sr25519Tag>;

/// Proof of Possession is the same as Signature for sr25519
pub type ProofOfPossession = Signature;

#[cfg(feature = "full_crypto")]
impl From<schnorrkel::Signature> for Signature {
	fn from(s: schnorrkel::Signature) -> Signature {
		Signature::from(s.to_bytes())
	}
}

/// An Schnorrkel/Ristretto x25519 ("sr25519") key pair.
pub struct Pair(Keypair);

impl Clone for Pair {
	fn clone(&self) -> Self {
		Pair(schnorrkel::Keypair {
			public: self.0.public,
			secret: schnorrkel::SecretKey::from_bytes(&self.0.secret.to_bytes()[..])
				.expect("key is always the correct size"),
		})
	}
}

#[cfg(feature = "std")]
impl From<MiniSecretKey> for Pair {
	fn from(sec: MiniSecretKey) -> Pair {
		Pair(sec.expand_to_keypair(ExpansionMode::Ed25519))
	}
}

#[cfg(feature = "std")]
impl From<SecretKey> for Pair {
	fn from(sec: SecretKey) -> Pair {
		Pair(Keypair::from(sec))
	}
}

#[cfg(feature = "full_crypto")]
impl From<schnorrkel::Keypair> for Pair {
	fn from(p: schnorrkel::Keypair) -> Pair {
		Pair(p)
	}
}

#[cfg(feature = "full_crypto")]
impl From<Pair> for schnorrkel::Keypair {
	fn from(p: Pair) -> schnorrkel::Keypair {
		p.0
	}
}

#[cfg(feature = "full_crypto")]
impl AsRef<schnorrkel::Keypair> for Pair {
	fn as_ref(&self) -> &schnorrkel::Keypair {
		&self.0
	}
}

/// Derive a single hard junction.
fn derive_hard_junction(secret: &SecretKey, cc: &[u8; CHAIN_CODE_LENGTH]) -> MiniSecretKey {
	secret.hard_derive_mini_secret_key(Some(ChainCode(*cc)), b"").0
}

/// The raw secret seed, which can be used to recreate the `Pair`.
type Seed = [u8; MINI_SECRET_KEY_LENGTH];

impl TraitPair for Pair {
	type Public = Public;
	type Seed = Seed;
	type Signature = Signature;
	type ProofOfPossession = ProofOfPossession;

	/// Get the public key.
	fn public(&self) -> Public {
		Public::from(self.0.public.to_bytes())
	}

	/// Make a new key pair from raw secret seed material.
	///
	/// This is generated using schnorrkel's Mini-Secret-Keys.
	///
	/// A `MiniSecretKey` is literally what Ed25519 calls a `SecretKey`, which is just 32 random
	/// bytes.
	fn from_seed_slice(seed: &[u8]) -> Result<Pair, SecretStringError> {
		match seed.len() {
			MINI_SECRET_KEY_LENGTH => Ok(Pair(
				MiniSecretKey::from_bytes(seed)
					.map_err(|_| SecretStringError::InvalidSeed)?
					.expand_to_keypair(ExpansionMode::Ed25519),
			)),
			SECRET_KEY_LENGTH => Ok(Pair(
				SecretKey::from_bytes(seed)
					.map_err(|_| SecretStringError::InvalidSeed)?
					.to_keypair(),
			)),
			_ => Err(SecretStringError::InvalidSeedLength),
		}
	}

	fn derive<Iter: Iterator<Item = DeriveJunction>>(
		&self,
		path: Iter,
		seed: Option<Seed>,
	) -> Result<(Pair, Option<Seed>), DeriveError> {
		let seed = seed
			.and_then(|s| MiniSecretKey::from_bytes(&s).ok())
			.filter(|msk| msk.expand(ExpansionMode::Ed25519) == self.0.secret);

		let init = self.0.secret.clone();
		let (result, seed) = path.fold((init, seed), |(acc, acc_seed), j| match (j, acc_seed) {
			(DeriveJunction::Soft(cc), _) => (acc.derived_key_simple(ChainCode(cc), &[]).0, None),
			(DeriveJunction::Hard(cc), maybe_seed) => {
				let seed = derive_hard_junction(&acc, &cc);
				(seed.expand(ExpansionMode::Ed25519), maybe_seed.map(|_| seed))
			},
		});
		Ok((Self(result.into()), seed.map(|s| MiniSecretKey::to_bytes(&s))))
	}

	#[cfg(feature = "full_crypto")]
	fn sign(&self, message: &[u8]) -> Signature {
		let context = signing_context(SIGNING_CTX);
		self.0.sign(context.bytes(message)).into()
	}

	fn verify<M: AsRef<[u8]>>(sig: &Signature, message: M, pubkey: &Public) -> bool {
		let Ok(signature) = schnorrkel::Signature::from_bytes(sig.as_ref()) else { return false };
		let Ok(public) = PublicKey::from_bytes(pubkey.as_ref()) else { return false };
		public.verify_simple(SIGNING_CTX, message.as_ref(), &signature).is_ok()
	}

	fn to_raw_vec(&self) -> Vec<u8> {
		self.0.secret.to_bytes().to_vec()
	}
}

#[cfg(not(substrate_runtime))]
impl Pair {
	/// Verify a signature on a message. Returns `true` if the signature is good.
	/// Supports old 0.1.1 deprecated signatures and should be used only for backward
	/// compatibility.
	pub fn verify_deprecated<M: AsRef<[u8]>>(sig: &Signature, message: M, pubkey: &Public) -> bool {
		// Match both schnorrkel 0.1.1 and 0.8.0+ signatures, supporting both wallets
		// that have not been upgraded and those that have.
		match PublicKey::from_bytes(pubkey.as_ref()) {
			Ok(pk) => pk
				.verify_simple_preaudit_deprecated(SIGNING_CTX, message.as_ref(), &sig.0[..])
				.is_ok(),
			Err(_) => false,
		}
	}
}

impl CryptoType for Public {
	type Pair = Pair;
}

impl CryptoType for Signature {
	type Pair = Pair;
}

impl CryptoType for Pair {
	type Pair = Pair;
}

impl NonAggregatable for Pair {}

/// Schnorrkel VRF related types and operations.
pub mod vrf {
	use super::*;
	#[cfg(feature = "full_crypto")]
	use crate::crypto::VrfSecret;
	use crate::crypto::{VrfCrypto, VrfPublic};
	use schnorrkel::{
		errors::MultiSignatureStage,
		vrf::{VRF_PREOUT_LENGTH, VRF_PROOF_LENGTH},
		SignatureError,
	};

	const DEFAULT_EXTRA_DATA_LABEL: &[u8] = b"VRF";

	/// Transcript ready to be used for VRF related operations.
	#[derive(Clone)]
	pub struct VrfTranscript(pub merlin::Transcript);

	impl VrfTranscript {
		/// Build a new transcript instance.
		///
		/// Each `data` element is a tuple `(domain, message)` used to build the transcript.
		pub fn new(label: &'static [u8], data: &[(&'static [u8], &[u8])]) -> Self {
			let mut transcript = merlin::Transcript::new(label);
			data.iter().for_each(|(l, b)| transcript.append_message(l, b));
			VrfTranscript(transcript)
		}

		/// Map transcript to `VrfSignData`.
		pub fn into_sign_data(self) -> VrfSignData {
			self.into()
		}
	}

	/// VRF input.
	///
	/// Technically a transcript used by the Fiat-Shamir transform.
	pub type VrfInput = VrfTranscript;

	/// VRF input ready to be used for VRF sign and verify operations.
	#[derive(Clone)]
	pub struct VrfSignData {
		/// Transcript data contributing to VRF output.
		pub(super) transcript: VrfTranscript,
		/// Extra transcript data to be signed by the VRF.
		pub(super) extra: Option<VrfTranscript>,
	}

	impl From<VrfInput> for VrfSignData {
		fn from(transcript: VrfInput) -> Self {
			VrfSignData { transcript, extra: None }
		}
	}

	// Get a reference to the inner VRF input.
	impl AsRef<VrfInput> for VrfSignData {
		fn as_ref(&self) -> &VrfInput {
			&self.transcript
		}
	}

	impl VrfSignData {
		/// Build a new instance ready to be used for VRF signer and verifier.
		///
		/// `input` will contribute to the VRF output bytes.
		pub fn new(input: VrfTranscript) -> Self {
			input.into()
		}

		/// Add some extra data to be signed.
		///
		/// `extra` will not contribute to the VRF output bytes.
		pub fn with_extra(mut self, extra: VrfTranscript) -> Self {
			self.extra = Some(extra);
			self
		}
	}

	/// VRF signature data
	#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
	pub struct VrfSignature {
		/// VRF pre-output.
		pub pre_output: VrfPreOutput,
		/// VRF proof.
		pub proof: VrfProof,
	}

	/// VRF pre-output type suitable for schnorrkel operations.
	#[derive(Clone, Debug, PartialEq, Eq)]
	pub struct VrfPreOutput(pub schnorrkel::vrf::VRFPreOut);

	impl Encode for VrfPreOutput {
		fn encode(&self) -> Vec<u8> {
			self.0.as_bytes().encode()
		}
	}

	impl Decode for VrfPreOutput {
		fn decode<R: codec::Input>(i: &mut R) -> Result<Self, codec::Error> {
			let decoded = <[u8; VRF_PREOUT_LENGTH]>::decode(i)?;
			Ok(Self(schnorrkel::vrf::VRFPreOut::from_bytes(&decoded).map_err(convert_error)?))
		}
	}

	impl MaxEncodedLen for VrfPreOutput {
		fn max_encoded_len() -> usize {
			<[u8; VRF_PREOUT_LENGTH]>::max_encoded_len()
		}
	}

	/// VRF proof type suitable for schnorrkel operations.
	#[derive(Clone, Debug, PartialEq, Eq)]
	pub struct VrfProof(pub schnorrkel::vrf::VRFProof);

	impl Encode for VrfProof {
		fn encode(&self) -> Vec<u8> {
			self.0.to_bytes().encode()
		}
	}

	impl Decode for VrfProof {
		fn decode<R: codec::Input>(i: &mut R) -> Result<Self, codec::Error> {
			let decoded = <[u8; VRF_PROOF_LENGTH]>::decode(i)?;
			Ok(Self(schnorrkel::vrf::VRFProof::from_bytes(&decoded).map_err(convert_error)?))
		}
	}

	impl MaxEncodedLen for VrfProof {
		fn max_encoded_len() -> usize {
			<[u8; VRF_PROOF_LENGTH]>::max_encoded_len()
		}
	}

	#[cfg(feature = "full_crypto")]
	impl VrfCrypto for Pair {
		type VrfInput = VrfTranscript;
		type VrfPreOutput = VrfPreOutput;
		type VrfSignData = VrfSignData;
		type VrfSignature = VrfSignature;
	}

	#[cfg(feature = "full_crypto")]
	impl VrfSecret for Pair {
		fn vrf_sign(&self, data: &Self::VrfSignData) -> Self::VrfSignature {
			let inout = self.0.vrf_create_hash(data.transcript.0.clone());

			let extra = data
				.extra
				.as_ref()
				.map(|e| e.0.clone())
				.unwrap_or_else(|| merlin::Transcript::new(DEFAULT_EXTRA_DATA_LABEL));

			let proof = self.0.dleq_proove(extra, &inout, true).0;

			VrfSignature { pre_output: VrfPreOutput(inout.to_preout()), proof: VrfProof(proof) }
		}

		fn vrf_pre_output(&self, input: &Self::VrfInput) -> Self::VrfPreOutput {
			let pre_output = self.0.vrf_create_hash(input.0.clone()).to_preout();
			VrfPreOutput(pre_output)
		}
	}

	impl VrfCrypto for Public {
		type VrfInput = VrfTranscript;
		type VrfPreOutput = VrfPreOutput;
		type VrfSignData = VrfSignData;
		type VrfSignature = VrfSignature;
	}

	impl VrfPublic for Public {
		fn vrf_verify(&self, data: &Self::VrfSignData, signature: &Self::VrfSignature) -> bool {
			let do_verify = || {
				let public = schnorrkel::PublicKey::from_bytes(&self.0)?;

				let inout =
					signature.pre_output.0.attach_input_hash(&public, data.transcript.0.clone())?;

				let extra = data
					.extra
					.as_ref()
					.map(|e| e.0.clone())
					.unwrap_or_else(|| merlin::Transcript::new(DEFAULT_EXTRA_DATA_LABEL));

				public.dleq_verify(extra, &inout, &signature.proof.0, true)
			};
			do_verify().is_ok()
		}
	}

	fn convert_error(e: SignatureError) -> codec::Error {
		use MultiSignatureStage::*;
		use SignatureError::*;
		match e {
			EquationFalse => "Signature error: `EquationFalse`".into(),
			PointDecompressionError => "Signature error: `PointDecompressionError`".into(),
			ScalarFormatError => "Signature error: `ScalarFormatError`".into(),
			NotMarkedSchnorrkel => "Signature error: `NotMarkedSchnorrkel`".into(),
			BytesLengthError { .. } => "Signature error: `BytesLengthError`".into(),
			InvalidKey => "Signature error: `InvalidKey`".into(),
			MuSigAbsent { musig_stage: Commitment } => {
				"Signature error: `MuSigAbsent` at stage `Commitment`".into()
			},
			MuSigAbsent { musig_stage: Reveal } => {
				"Signature error: `MuSigAbsent` at stage `Reveal`".into()
			},
			MuSigAbsent { musig_stage: Cosignature } => {
				"Signature error: `MuSigAbsent` at stage `Commitment`".into()
			},
			MuSigInconsistent { musig_stage: Commitment, duplicate: true } => {
				"Signature error: `MuSigInconsistent` at stage `Commitment` on duplicate".into()
			},
			MuSigInconsistent { musig_stage: Commitment, duplicate: false } => {
				"Signature error: `MuSigInconsistent` at stage `Commitment` on not duplicate".into()
			},
			MuSigInconsistent { musig_stage: Reveal, duplicate: true } => {
				"Signature error: `MuSigInconsistent` at stage `Reveal` on duplicate".into()
			},
			MuSigInconsistent { musig_stage: Reveal, duplicate: false } => {
				"Signature error: `MuSigInconsistent` at stage `Reveal` on not duplicate".into()
			},
			MuSigInconsistent { musig_stage: Cosignature, duplicate: true } => {
				"Signature error: `MuSigInconsistent` at stage `Cosignature` on duplicate".into()
			},
			MuSigInconsistent { musig_stage: Cosignature, duplicate: false } => {
				"Signature error: `MuSigInconsistent` at stage `Cosignature` on not duplicate"
					.into()
			},
		}
	}

	#[cfg(feature = "full_crypto")]
	impl Pair {
		/// Generate output bytes from the given VRF configuration.
		pub fn make_bytes<const N: usize>(&self, context: &[u8], input: &VrfInput) -> [u8; N]
		where
			[u8; N]: Default,
		{
			let inout = self.0.vrf_create_hash(input.0.clone());
			inout.make_bytes::<[u8; N]>(context)
		}
	}

	impl Public {
		/// Generate output bytes from the given VRF configuration.
		pub fn make_bytes<const N: usize>(
			&self,
			context: &[u8],
			input: &VrfInput,
			pre_output: &VrfPreOutput,
		) -> Result<[u8; N], codec::Error>
		where
			[u8; N]: Default,
		{
			let pubkey = schnorrkel::PublicKey::from_bytes(&self.0).map_err(convert_error)?;
			let inout = pre_output
				.0
				.attach_input_hash(&pubkey, input.0.clone())
				.map_err(convert_error)?;
			Ok(inout.make_bytes::<[u8; N]>(context))
		}
	}

	impl VrfPreOutput {
		/// Generate output bytes from the given VRF configuration.
		pub fn make_bytes<const N: usize>(
			&self,
			context: &[u8],
			input: &VrfInput,
			public: &Public,
		) -> Result<[u8; N], codec::Error>
		where
			[u8; N]: Default,
		{
			public.make_bytes(context, input, self)
		}
	}
}

#[cfg(test)]
mod tests {
}
