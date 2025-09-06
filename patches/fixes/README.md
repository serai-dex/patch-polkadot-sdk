# Fixes for the `polkadot-sdk`

- `substrate-wasm-builder` corrects `substrate-wasm-builder` to not propagate
  the `CARGO_FEATURE_STD` environment variable, which prevents crates dependent
  on `cfg_aliases` for determining if it's `std` from compiling (as
  `cfg_aliases` will believe it's in an `std`-enabled environment due to the
  environment variable).
