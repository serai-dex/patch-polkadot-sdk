# Opinionated Changes

- `events_on_genesis`: Updates `frame-system` to emit events on genesis, which
  `polkadot-sdk` disabled due to the size of some genesis configurations (with
  millions of transfer events for initial distributions and so on). Also
  introduces a `panic` if the amount of events exceeds `2**32`, where
  `polkadot-sdk` would begin silently dropping events. This risks a chain stall
  for giant blocks to ensure the event log is perfectly accurate.

- `zero_genesis_hash`: Updates `frame-system` to use `[0; _]` for the current
  block hash when building the genesis block itself.

- `scale_genesis_config`: Removes `serde::{Serialize, Deserialize}` from the
  genesis config in favor of `scale::{Encode, Decode}`, allowing non-JSON
  `GenesisConfig`s.

- `session`: Replaces `pallet-session` with a much more minimal variant,
  sufficient for `pallet-babe` and `pallet-grandpa`.

- `wasm32v1-none`: Mandates use of `wasm32v1-none` without falling back to
  `wasm32-unknown-unknown`.
