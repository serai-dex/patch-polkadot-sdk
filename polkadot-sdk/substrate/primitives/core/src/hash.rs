#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct H256([u8; 32]);

impl From<[u8; 32]> for H256 {
  fn from(bytes: [u8; 32]) -> Self {
    Self(bytes)
  }
}
impl From<H256> for [u8; 32] {
  fn from(hash: H256) -> Self {
    hash.0
  }
}

impl core::convert::AsRef<[u8]> for H256 {
  fn as_ref(&self) -> &[u8] {
    self.0.as_slice()
  }
}
impl core::convert::AsMut<[u8]> for H256 {
  fn as_mut(&mut self) -> &mut [u8] {
    self.0.as_mut_slice()
  }
}

impl core::fmt::Debug for H256 {
  fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Debug::fmt(&crate::hexdisplay::HexDisplay::from(&self.0), fmt)
  }
}
impl core::fmt::Display for H256 {
  fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&crate::hexdisplay::HexDisplay::from(&self.0), fmt)
  }
}
impl core::str::FromStr for H256 {
  type Err = ();
  fn from_str(str: &str) -> Result<Self, Self::Err> {
    let mut res = [0; 32];
    hex::decode_to_slice(str.as_bytes(), &mut res).map_err(|_| ())?;
    Ok(Self(res))
  }
}

impl codec::Encode for H256 {
  fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R { f(self.0.as_slice()) }
}
impl codec::Decode for H256 {
  fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
    let mut res = [0; 32];
    input.read(&mut res)?;
    Ok(Self(res))
  }
}
impl codec::DecodeWithMemTracking for H256 {}
impl codec::EncodeLike for H256 {}
impl codec::MaxEncodedLen for H256 {
  fn max_encoded_len() -> usize { 32 }
}
