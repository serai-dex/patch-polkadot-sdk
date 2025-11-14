# Patches

None of these patches are guaranteed to work on their own, solely as
orchestrated by `patch.sh`. They may require further edits performed by
`patch.sh` or require other patches present.

These patches primarily prioritize _stability_. They should not be affected by
the addition/removal of other patches, nor by updating the `polkadot-sdk`
version derived from. If they are broken, they should be quick to correct.

File removals are preferred to be done via `rm`, not via patch files, as `git`
will consider updates to removed files as conflicts when the purpose is to
remove them entirely.

- `features`: Features added on top.

- `fixes`: Bug fixes.

- `metadata`: Removes `scale_info::TypeInfo` and metadata from the Serai
  protocol, which isn't intended to be a parachain dynamically connected to. We
  have a specific, bespoke API requiring interfaces be tailored to us. The
  metadata would be at best irrelevant and a waste of space, yet practically
  would be incorrect and not only unhelpful, yet harmful.

- `opinions`: Tweaks to behavior per Serai's opinions.

- `optional_polkavm`: Makes `polkavm` (and related crates) optional
  dependencies. This prevents the tree from including it, when it's
  experimental code not recommended for use at this time. Overtime, `polkavm`
  will become Serai's preferred backend however (due to being premised on
  RISC-V, not WASM, and having a much smaller tree than `wasmtime`).

- `optional_litep2p`: Makes `litep2p` an optional dependency. This is strongly
  opinionated as `litep2p` is the _recommended_ network backend and will become
  the only backend with a maintenance guarantee. Serai prefers it to be
  optional as Serai independently uses `libp2p`, and prefers solely having
  `libp2p` in-tree at this time.

- `removals`: Patch to remove references to removed crates/functionality.

- `std`: Patches which replace external dependencies with usage of the standard
  library.

- `updates`: Patches to update dependencies across major versions, resolving
  breaking changes.
