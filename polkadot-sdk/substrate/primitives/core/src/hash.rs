fixed_hash::construct_fixed_hash! {
  pub struct H256(32);
}
impl_codec::impl_fixed_hash_codec!(H256, 32);
impl_serde::impl_fixed_hash_serde!(H256, 32);
