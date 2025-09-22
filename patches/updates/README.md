# Updates

- `async-channel`: Updates from `async-channel 1` to `async-channel 2`.

- `governor`: Updates from `governor 0.6` to `governor 0.10`.

- `libp2p`: Allows using either `libp2p 0.54` or `libp2p 0.55` when `void` is
  patched to the crate within this repository. The caller is expected to ensure
  `libp2p-kad` is kept in sync with their choice of `libp2p`. Newer versions of
  `libp2p` may fail to build when the `litep2p` feature is enabled.

- `prometheus`: Updates from `prometheus 0.13` to `prometheus 0.14`.

- `twox-hash`: Updates from `twox-hash 1` to `twox-hash 2`.

- `wasmtime`: Updates from `wasmtime 8` to `wasmtime 36`, bringing `rustix 1` alone.
