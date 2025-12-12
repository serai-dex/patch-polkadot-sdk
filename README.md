# serai-polkadot-sdk

A script to generate the `polkadot-sdk` in the optimal form for Serai.

# Details

This folder contains a Rust binary (`serai-polkadot-sdk`) which provides
required functionality (automatic updating of all `Cargo.toml`s within a
folder) to produce our `polkadot-sdk` fork. It's called by `patch.sh`.

`patch.sh` orchestrates the derivation process, applies some patches directly
using `bash`, and applies some patches via patch files present in
[`/patches`](patches/). Please note the patches in `/patches` are not
guaranteed to work independently.

Neither the `serai-polkadot-sdk` binary nor `patch.sh` script are intended to
be wholly and entirely accurate, in every case. Instead, they're sufficiently
accurate for the expressed goal _with each transformation having been reviewed
to ensure it was proper_. This means even without fully lexing the Rust syntax,
some transformations are checked to be applied properly for this specific
use-case.

The primary goal is simply to minimize the `polkadot-sdk` tree. This script
leaves roughly just a third of the dependencies in use standing, and the result
still takes tens of minutes to be checked, producing a `target/` exceeding
10 GB _without producing any binaries_.

While removals may be over-eager, the point is removed crates may be quickly
restored as needed, if needed. Accordingly, there's no harm to being aggressive
now. The methodology also is intended to allow quickly updating the
`polkadot-sdk` version derived from.

# Result

`patch.sh` ensures the result compiles with `cargo check --all-features`. No
guarantees are made to the documentation nor tests.

# Licensing

The `polkadot-sdk` folder included in this repository is the artifact output
from running the script. It is a `git clone` of the `paritytech/polkadot-sdk`
repository, with many removals and light patches applied. `polkadot-sdk`, as
originally cloned, is as licensed by Parity (with their license and copyright
statements fully intact, preserved in the derivative produced). The patches
present in `patches`, the transformations produced by `serai-polkadot-sdk` and
`patch.sh`, and `serai-polkadot-sdk` and `patch.sh` themselves are licensed
under the AGPL 3.0 only, with no grant to use future revisions of the AGPL
license, or under the same licensing conditions as the files patched as
necessary to satisfy copyleft license requirements. A copy of the AGPL and
short license texts are included within this repository.

The `.github` folder is published under the MIT license. Please see
`.github/LICENSE` for more information.
