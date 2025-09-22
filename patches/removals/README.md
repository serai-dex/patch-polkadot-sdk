# Removals

- `aura`: Removes references to Aura consensus.

- `BinaryMerkleTreProver`: Enables removing the `binary-merkle-tree` crate.

- `bitswap`: Removes support for the `bitswap` protocol due to lack of use by
  Serai, and lack of desire to potentially use
  (https://github.com/libp2p/rust-libp2p/issues/2632).

- `docify`: Removes references to `docify`. `patch.sh` also removes
  `aquamarine`, `simple-mermaid`.

- `ecdsa_ed25519`: Removes ECDSA, Ed25519 cryptography from the runtime.

- `frame-system-benchmarking`: Removes references to
  `frame-system-benchmarking`.

- `NativeExecutor`: Removes `NativeExecutor`, `NativeElseWasmExecutor`,
  which were deprecated and were supposed to be removed at the end of 2024.

- `offchain_http`: Removes HTTP functionality from `sp-offchain`,
  `sc-offchain`, etc. This allows removing a variety of crates for HTTP from
  the tree.

- `sp-transaction-storage-proof`: Removes references to
  `sp-transaction-storage`.

- `SS58Prefix`: Removes `SS58Prefix`, as Serai prefers `bech32` and doesn't
  need it as a chain ID. Note this patch is partnered by a blanket removal of
  all lines mentioning `SS58Prefix` (manually reviewed) which is known to bork
  some docstrings.

- `substrate-bip39`: Removes `substrate-bip39` from usage. `substrate-bip39`
  was forked from `tiny-bip39` as it was unmaintained, yet for some reason,
  Parity decided to build `parity-bip39` (another fork of `tiny-bip39`).
  Instead of replacing `substrate-bip39` with it, they proceeded to use both
  simultaneously, which this patches.

  Note this patch claims provided passwords are already normalized UTF-8, when
  they may not be. As Serai doesn't plan to use Substrate's provided seed
  management, and maintained it solely as it's simpler than removal, this is
  irrelevant to Serai yet should be watched out for.

- `sysinfo`: Removes `sysinfo` which was only included to print a warning in
  certain conditions.

- `wasm-opt`: Removes `wasm-opt` whose requirement has been largely superseded
  by the introduction of `wasm32v1-none` and `-Zbuild-std`.

- `wasm-timer`: Removes support for WASM from `sc-network`, `sc-telemtry`.
