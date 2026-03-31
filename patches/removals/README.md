# Removals

- `bitswap`: Removes support for the `bitswap` protocol due to lack of use by
  Serai, and lack of desire to potentially use
  (https://github.com/libp2p/rust-libp2p/issues/2632).

- `crate_version`: Remove the `crate_version` metadata from pallets.

- `ecdsa_ed25519`: Removes ECDSA, Ed25519 cryptography from the runtime.

- `fixed_point`: Remove the `sp_arithmetic::fixed_point` module.

- `light_client`: Removes the unused light-client request server.

- `memmap2`: Remove use of the `memmap2` crate.

- `metadata`: Removes `scale_info::TypeInfo` and metadata from `frame-support`
  and the rest of Substrate.

- `NativeExecutor`: Removes `NativeExecutor`, `NativeElseWasmExecutor`,
  which were deprecated and were supposed to be removed at the end of 2024.

- `offchain_http`: Removes HTTP functionality from `sp-offchain`,
  `sc-offchain`, etc. This allows removing a variety of crates for HTTP from
  the tree.

- `rpc_modules`: Removes provided RPC modules.

- `SessionKeys`: Removes `sp_session::runtime_api::SessionKeys`.

- `sp-genesis-builder`: Removes the JSON-premised genesis code.

- `sp-maybe-compressed-blob`: Remove compression of the on-chain code.

- `sp-panic-handler`: Removes Substrate's bespoke panic handler which only had
  two references in the resulting codebase. The first was to force aborting the
  process, when Serai is fundamentally built with `panic = "abort"`, making it
  unnecessary. The second was for unwinding upon runtime panics, which should
  be unreachable now that native execution was removed, all execution of the
  runtime is via WASM, and the runtime panicking will call `wasmtime` to yield
  an error (not propagate the panic).

- `sp-transaction-storage-proof`: Removes references to
  `sp-transaction-storage`.

- `SS58Prefix`: Removes `SS58Prefix`.

- `substrate-bip39`: Removes `substrate-bip39` from usage. `substrate-bip39`
  was forked from `tiny-bip39` as it was unmaintained, yet for some reason,
  Parity decided to build `parity-bip39` (another fork of `tiny-bip39`).
  Instead of replacing `substrate-bip39` with it, they proceeded to use both
  simultaneously, which this patches.

  Note this patch claims provided passwords are already normalized UTF-8, when
  they may not be. As Serai doesn't plan to use Substrate's provided seed
  management, and maintained it solely as it's simpler than removal, this is
  irrelevant to Serai yet should be watched out for.

- `substrate-prometheus-endpoint`: Neuters, but doesn't outright remove,
  `substrate-prometheus-endpoint`.

- `sysinfo`: Removes `sysinfo` which was only included to print a warning in
  certain conditions.

- `task`: Finishes removing `Task` from `frame-support`, already partially done
  with `composite`.

- `unused-frame-support`: Removes legacy code unused within the current result.

- `wasm-instrument`: Removes `wasm-instrument`, and with it the unmaintained
  `parity-wasm` (which may be complete yet also does not accept issues), via
  removing the functionality it offers (unnecessary for WASM blob of trusted
  origin).

- `wasm-timer`: Removes support for WASM from `sc-network`, `sc-telemtry`.

- `wasmtime-caching`: Removes usage of `wasmtime`'s cache feature. This is due
  to Serai having a single runtime (its own) and not needing filesystem-level
  caching, solely in-memory caching of the most recent version (or one other
  version, if an upgrade is approximate).
