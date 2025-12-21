# Features

- `disable_inherent`: Extends the `runtime` macro to allow disabling pallets'
  inherents.

- `optional-construct_runtime`: This moves `frame_support::construct_runtime`,
  deprecated in favor of `frame_support::runtime`, behind
  `cfg(debug_assertions)`. While `frame_support::construct_runtime` is
  still used within tests, hence it still being available when writing tests,
  it is _only_ used within tests yet still mandates `tt_default_parts`
  (whereas `frame_support::runtime` uses `tt_default_parts_v2`) which is ~10%
  of the entire macro expansion. This causes a measurable benefit to
  compilation times.

- `optional-litep2p`: Makes `litep2p` an optional dependency.

- `optional-polkavm`: Makes `polkavm` an optional dependency.
