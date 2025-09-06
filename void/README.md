# void shim

A shim for `void` which maps to `std::convert::Infallible`.

With this used as a patch for `void`, the contained `polkadot-sdk` may be used
with `libp2p 0.55`.
