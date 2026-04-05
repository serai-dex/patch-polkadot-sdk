uint::construct_uint! {
  pub struct U256(4);
}
impl_codec::impl_uint_codec!(U256, 4);
impl_serde::impl_uint_serde!(U256, 4);
