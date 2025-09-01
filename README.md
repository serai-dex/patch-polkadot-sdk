# serai-polkadot-sdk

A script to generate the `polkadot-sdk` in the optimal form for Serai.

# Details

The script, `patch.sh`, makes uses of a Rust binary (`serai-polkadot-sdk`), to
accomplish its goal. Functionality is split between the two of them. The
`patches` folder contains a set of patches we apply to `polkadot-sdk`. Note
that not all changes are accomplished via the `patches` folder, and some
modifications are inlined into the `patch.sh` script.

THe primary goal is simply to minimize the `polkadot-sdk` tree. This script
more than halves the amount of dependencies in use, and the result still takes
tens of minutes to be checked, producing a target exceeding 10 GB
_without producing any binaries_.

While removals may be over-eager, the point is removed crates may be quickly
restored as needed, if needed. Accordingly, there's no harm to being aggressive
now.

# Licensing

The `polkadot-sdk` folder included in this repository is the artifact output
from running the script. It is a `git clone` of the `paritytech/polkadot-sdk`
repository, with many removals and light patches applied. `polkadot-sdk`, as
originally cloned, is as licensed by Parity (with their license and copyright
statements fully intact, preserved in the derivative produced). The patches
present in `patches`, the transformations of `serai-polkadot-sdk` and
`patch.sh`, and `serai-polkadot-sdk` and `patch.sh` themselves are licensed
under the AGPL 3.0 only, with no grant to use future revisions of the AGPL
license. A copy of the AGPL and short license texts are included within this
repository.

The `.github` folder is published under the MIT license. Please see
`.github/LICENSE` for more information.
