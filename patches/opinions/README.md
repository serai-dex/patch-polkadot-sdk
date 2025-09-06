# Opinionated Changes

- `events_on_genesis`: Updates `frame-system` to emit events on genesis, which
  `polkadot-sdk` disabled due to the size of some genesis configurations (with
  millions of transfer events for initial distributions and so on). Also
  introduces a `panic` if the amount of events exceeds `2**32`, where
  `polkadot-sdk` would begin silently dropping events. This risks a chain stall
  for giant blocks to ensure the event log is perfectly accurate.
