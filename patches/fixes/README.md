# Fixes for the `polkadot-sdk`

- `substrate-wasm-builder`: Fixes `substrate-wasm-builder` to not propagate the
  `CARGO_FEATURE_STD` environment variable, which prevents crates dependent on
  `cfg_aliases` for determining if it's `std` from compiling (as `cfg_aliases`
  will believe it's in an `std`-enabled environment due to the environment
  variable).

- `sc-basic-authorship`: Has `sc-basic-authorship` check if an extrinsic was
  mandatory before checking if resources were exhausted. Then, if a mandatory
  extrinsic is ever flagged as exhausting resources, it panics as intended
  instead of being dropped as if non-mandatory.
