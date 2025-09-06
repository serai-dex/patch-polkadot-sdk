# Updates

- `governor`: Updates from `governor 0.6` to `governor 0.10`.

- `prometheus`: Updates from `prometheus 0.13` to `prometheus 0.14`.

- `twox-hash`: Updates from `twox-hash 1` to `twox-hash 2`.

- `wasm-timer`: Updates `wasm-timer` to the fork `wasmtimer`. This was done as
  Serai had both in tree (as `alloy` depends on `wasmtimer`) and could
  reconcile `wasm-timer` here (yet not vice versa). `wasmtimer` is also a
  slightly slimmer package.

- `wasmtime`: Updates from `wasmtime 8` to `wasmtime 36`, bringing `rustix 1` alone.
