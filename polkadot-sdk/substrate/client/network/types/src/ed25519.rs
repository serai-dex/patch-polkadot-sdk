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

//! Ed25519 keys.

use crate::PeerId;
use core::{cmp, fmt, hash};
use ed25519_dalek::{self as ed25519, Signer as _, Verifier as _};
use libp2p_identity::ed25519 as libp2p_ed25519;
#[cfg(feature = "litep2p")]
use litep2p::crypto::ed25519 as litep2p_ed25519;
use zeroize::Zeroize;

/// An Ed25519 keypair.
#[derive(Clone)]
pub struct Keypair(ed25519::SigningKey);

impl Keypair {
	/// Generate a new random Ed25519 keypair.
	pub fn generate() -> Keypair {
		Keypair::from(SecretKey::generate())
	}

	/// Convert the keypair into a byte array by concatenating the bytes
	/// of the secret scalar and the compressed public point,
	/// an informal standard for encoding Ed25519 keypairs.
	pub fn to_bytes(&self) -> [u8; 64] {
		self.0.to_keypair_bytes()
	}

	/// Try to parse a keypair from the [binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)
	/// produced by [`Keypair::to_bytes`], zeroing the input on success.
	///
	/// Note that this binary format is the same as `ed25519_dalek`'s and `ed25519_zebra`'s.
	pub fn try_from_bytes(kp: &mut [u8]) -> Result<Keypair, DecodingError> {
		let bytes = <[u8; 64]>::try_from(&*kp)
			.map_err(|e| DecodingError::KeypairParseError(Box::new(e)))?;

		ed25519::SigningKey::from_keypair_bytes(&bytes)
			.map(|k| {
				kp.zeroize();
				Keypair(k)
			})
			.map_err(|e| DecodingError::KeypairParseError(Box::new(e)))
	}

	/// Sign a message using the private key of this keypair.
	pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
		self.0.sign(msg).to_bytes().to_vec()
	}

	/// Get the public key of this keypair.
	pub fn public(&self) -> PublicKey {
		PublicKey(self.0.verifying_key())
	}

	/// Get the secret key of this keypair.
	pub fn secret(&self) -> SecretKey {
		SecretKey(self.0.to_bytes())
	}
}

impl fmt::Debug for Keypair {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Keypair").field("public", &self.0.verifying_key()).finish()
	}
}

#[cfg(feature = "litep2p")]
impl From<litep2p_ed25519::Keypair> for Keypair {
	fn from(kp: litep2p_ed25519::Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("ed25519_dalek in substrate & litep2p to use the same format")
	}
}

#[cfg(feature = "litep2p")]
impl From<Keypair> for litep2p_ed25519::Keypair {
	fn from(kp: Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("ed25519_dalek in substrate & litep2p to use the same format")
	}
}

impl From<libp2p_ed25519::Keypair> for Keypair {
	fn from(kp: libp2p_ed25519::Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("ed25519_dalek in substrate & libp2p to use the same format")
	}
}

impl From<Keypair> for libp2p_ed25519::Keypair {
	fn from(kp: Keypair) -> Self {
		Self::try_from_bytes(&mut kp.to_bytes())
			.expect("ed25519_dalek in substrate & libp2p to use the same format")
	}
}

/// Demote an Ed25519 keypair to a secret key.
impl From<Keypair> for SecretKey {
	fn from(kp: Keypair) -> SecretKey {
		SecretKey(kp.0.to_bytes())
	}
}

/// Promote an Ed25519 secret key into a keypair.
impl From<SecretKey> for Keypair {
	fn from(sk: SecretKey) -> Keypair {
		let signing = ed25519::SigningKey::from_bytes(&sk.0);
		Keypair(signing)
	}
}

/// An Ed25519 public key.
#[derive(Eq, Clone)]
pub struct PublicKey(ed25519::VerifyingKey);

impl fmt::Debug for PublicKey {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("PublicKey(compressed): ")?;
		for byte in self.0.as_bytes() {
			write!(f, "{byte:x}")?;
		}
		Ok(())
	}
}

impl cmp::PartialEq for PublicKey {
	fn eq(&self, other: &Self) -> bool {
		self.0.as_bytes().eq(other.0.as_bytes())
	}
}

impl hash::Hash for PublicKey {
	fn hash<H: hash::Hasher>(&self, state: &mut H) {
		self.0.as_bytes().hash(state);
	}
}

impl cmp::PartialOrd for PublicKey {
	fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl cmp::Ord for PublicKey {
	fn cmp(&self, other: &Self) -> cmp::Ordering {
		self.0.as_bytes().cmp(other.0.as_bytes())
	}
}

impl PublicKey {
	/// Verify the Ed25519 signature on a message using the public key.
	pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
		ed25519::Signature::try_from(sig).and_then(|s| self.0.verify(msg, &s)).is_ok()
	}

	/// Convert the public key to a byte array in compressed form, i.e.
	/// where one coordinate is represented by a single bit.
	pub fn to_bytes(&self) -> [u8; 32] {
		self.0.to_bytes()
	}

	/// Try to parse a public key from a byte array containing the actual key as produced by
	/// `to_bytes`.
	pub fn try_from_bytes(k: &[u8]) -> Result<PublicKey, DecodingError> {
		let k =
			<[u8; 32]>::try_from(k).map_err(|e| DecodingError::PublicKeyParseError(Box::new(e)))?;
		ed25519::VerifyingKey::from_bytes(&k)
			.map_err(|e| DecodingError::PublicKeyParseError(Box::new(e)))
			.map(PublicKey)
	}

	/// Convert public key to `PeerId`.
	pub fn to_peer_id(&self) -> PeerId {
		libp2p_identity::PeerId::from_public_key(&libp2p_ed25519::PublicKey::from(self.clone()).into()).into()
	}
}

#[cfg(feature = "litep2p")]
impl From<litep2p_ed25519::PublicKey> for PublicKey {
	fn from(k: litep2p_ed25519::PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes())
			.expect("ed25519_dalek in substrate & litep2p to use the same format")
	}
}

#[cfg(feature = "litep2p")]
impl From<PublicKey> for litep2p_ed25519::PublicKey {
	fn from(k: PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes())
			.expect("ed25519_dalek in substrate & litep2p to use the same format")
	}
}

impl From<libp2p_ed25519::PublicKey> for PublicKey {
	fn from(k: libp2p_ed25519::PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes())
			.expect("ed25519_dalek in substrate & libp2p to use the same format")
	}
}

impl From<PublicKey> for libp2p_ed25519::PublicKey {
	fn from(k: PublicKey) -> Self {
		Self::try_from_bytes(&k.to_bytes())
			.expect("ed25519_dalek in substrate & libp2p to use the same format")
	}
}

/// An Ed25519 secret key.
#[derive(Clone)]
pub struct SecretKey(ed25519::SecretKey);

/// View the bytes of the secret key.
impl AsRef<[u8]> for SecretKey {
	fn as_ref(&self) -> &[u8] {
		&self.0[..]
	}
}

impl fmt::Debug for SecretKey {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "SecretKey")
	}
}

impl SecretKey {
	/// Generate a new Ed25519 secret key.
	pub fn generate() -> SecretKey {
		let signing = ed25519::SigningKey::generate(&mut rand::rngs::OsRng);
		SecretKey(signing.to_bytes())
	}

	/// Try to parse an Ed25519 secret key from a byte slice
	/// containing the actual key, zeroing the input on success.
	/// If the bytes do not constitute a valid Ed25519 secret key, an error is
	/// returned.
	pub fn try_from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
		let sk_bytes = sk_bytes.as_mut();
		let secret = <[u8; 32]>::try_from(&*sk_bytes)
			.map_err(|e| DecodingError::SecretKeyParseError(Box::new(e)))?;
		sk_bytes.zeroize();
		Ok(SecretKey(secret))
	}

	pub fn to_bytes(&self) -> [u8; 32] {
		self.0
	}
}

impl Drop for SecretKey {
	fn drop(&mut self) {
		self.0.zeroize();
	}
}

#[cfg(feature = "litep2p")]
impl From<litep2p_ed25519::SecretKey> for SecretKey {
	fn from(sk: litep2p_ed25519::SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.to_bytes()).expect("Ed25519 key to be 32 bytes length")
	}
}

#[cfg(feature = "litep2p")]
impl From<SecretKey> for litep2p_ed25519::SecretKey {
	fn from(sk: SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.to_bytes())
			.expect("litep2p `SecretKey` to accept 32 bytes as Ed25519 key")
	}
}

impl From<libp2p_ed25519::SecretKey> for SecretKey {
	fn from(sk: libp2p_ed25519::SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.as_ref().to_owned())
			.expect("Ed25519 key to be 32 bytes length")
	}
}

impl From<SecretKey> for libp2p_ed25519::SecretKey {
	fn from(sk: SecretKey) -> Self {
		Self::try_from_bytes(&mut sk.to_bytes())
			.expect("libp2p `SecretKey` to accept 32 bytes as Ed25519 key")
	}
}

/// Error when decoding `ed25519`-related types.
#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
	#[error("failed to parse Ed25519 keypair: {0}")]
	KeypairParseError(Box<dyn std::error::Error + Send + Sync>),
	#[error("failed to parse Ed25519 secret key: {0}")]
	SecretKeyParseError(Box<dyn std::error::Error + Send + Sync>),
	#[error("failed to parse Ed25519 public key: {0}")]
	PublicKeyParseError(Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
}
