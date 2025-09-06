function silent_rm {
  $(rm -rf $1)
  return 0
}

# Start by checking out the desired version of the polkadot-sdk

POLKADOT_SDK_COMMIT=52f4a08f26f226de93c0dbea5e8d066cbbd5bbd0

if [ -f "./polkadot-sdk/.patched" ]; then
  if [ ! "$1" = "--from-scratch" ]; then
    echo "\`polkadot-sdk\` was already patched. Run with \`--from-scratch\` to continue"
    exit 1
  fi
fi

# If we're running this script yet `polkadot-sdk` isn't a valid Git repository, clean it
if [ -d "polkadot-sdk" ]; then
  if [ ! -d "polkadot-sdk/.git" ]; then
    silent_rm ./polkadot-sdk
  fi
fi

# Clone `polkadot-sdk`, yet only the specific commit we're patching
# Ideally, this would be `git clone --revision $POLKADOT_SDK_COMMIT --depth 1`,
# yet that requires a newer Git than frequently packaged
if [ ! -d "polkadot-sdk" ]; then
  mkdir ./polkadot-sdk
  cd ./polkadot-sdk
  git init
  git remote add origin https://github.com/paritytech/polkadot-sdk
  git fetch --depth 1 origin $POLKADOT_SDK_COMMIT
  cd ..
fi

cd ./polkadot-sdk
# Ensure we're starting from the intended commit
git checkout -f $POLKADOT_SDK_COMMIT
# Remove the existing `.patched` marker
silent_rm .patched
cd ..

# Our binary, when it makes its modifications, will overwrite all existing
# `Cargo.toml` files until they're unrecognizable. We start by making changes
# _to_ the `Cargo.toml` files accordingly

function apply_patch {
  echo "Applying patch $1"
  cd ./polkadot-sdk
  git apply ../patches/$1.patch
  PATCH_SUCCEEDED=$?
  cd ..
  if [ $PATCH_SUCCEEDED -ne 0 ]; then
    exit 2
  fi
}

function apply_patch_dir {
  ls patches/$1 | grep -v "README\.md" | while read -r patch; do
    apply_patch $1/$(echo $patch | sed s/".patch"//)
  done
  PATCHES_SUCCEEDED=$?
  if [ $PATCHES_SUCCEEDED -ne 0 ]; then
    exit $PATCHES_SUCCEEDED
  fi
}

function remove_matching_lines {
  ORIGINAL=$(cat $1)
  STRIPPED=$(echo "$ORIGINAL" | grep -v "$2")
  echo "$STRIPPED" > $1
}

# Apply the patches which are bug fixes
apply_patch_dir fixes
# Apply the patches which replace dependencies in favor of `std`
apply_patch_dir std
# Apply the patches which perform dependency updates
apply_patch_dir updates
# Make `litep2p`, `polkavm` optional
apply_patch optional_litep2p
apply_patch_dir optional_polkavm

# Replace the `wasm-timer` dependency with `wasmtimer`
remove_matching_lines ./polkadot-sdk/substrate/client/telemetry/Cargo.toml "wasm-timer"
echo '[dependencies.wasmtimer]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'version = "0.4"' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'default-features = false' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'features = ["tokio"]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "wasm-timer"
echo '[dependencies.wasmtimer]' >> ./polkadot-sdk/substrate/client/network/Cargo.toml
echo 'version = "0.4"' >> ./polkadot-sdk/substrate/client/network/Cargo.toml
echo 'default-features = false' >> ./polkadot-sdk/substrate/client/network/Cargo.toml
echo 'features = ["tokio"]' >> ./polkadot-sdk/substrate/client/network/Cargo.toml

# Remove unused HTTP module and associated dependencies
silent_rm ./polkadot-sdk/substrate/primitives/runtime/src/offchain/http.rs
silent_rm ./polkadot-sdk/substrate/client/offchain/src/api/http.rs
apply_patch removals/offchain_http

# Remove various unused dependencies
remove_matching_lines ./polkadot-sdk/substrate/client/chain-spec/Cargo.toml "memmap2"
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "cid"
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "prost"
remove_matching_lines ./polkadot-sdk/substrate/client/service/Cargo.toml "static_init"
remove_matching_lines ./polkadot-sdk/substrate/client/tracing/Cargo.toml "is-terminal"
remove_matching_lines ./polkadot-sdk/substrate/frame/support/Cargo.toml "frame-metadata"
remove_matching_lines ./polkadot-sdk/substrate/frame/support/Cargo.toml "k256"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "ark-vrf"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "ed25519-zebra"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "libsecp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "secp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "k256"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "w3f-bls"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "ed25519-dalek"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "libsecp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "secp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/Cargo.toml "schemars"
remove_matching_lines ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml "build-helper"
remove_matching_lines ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml "frame-metadata"
remove_matching_lines ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml "merkleized-metadata"

# Remove `simple-mermaid`
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/src/generic/unchecked_extrinsic.rs "simple_mermaid"
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/Cargo.toml "simple-mermaid"
remove_matching_lines ./polkadot-sdk/Cargo.toml "simple-mermaid"

# Remove `docify` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "docify"); echo "$STRIPPED" > {}' \;
remove_matching_lines ./polkadot-sdk/Cargo.toml "docify"

# Remove `aquamarine` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "aquamarine"); echo "$STRIPPED" > {}' \;
remove_matching_lines ./polkadot-sdk/Cargo.toml "aquamarine"

# Remove `is-terminal`
find ./polkadot-sdk/substrate/client/tracing -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed s/"is_terminal::IsTerminal"/"std::io::IsTerminal"/); echo "$STRIPPED" > {}' \;

# Remove `build-helper`
apply_patch removals/build-helper

# Remove `dyn-clonable` for `dyn-clone`, where both are already in-use within this tree
echo "$(cat ./polkadot-sdk/substrate/primitives/core/Cargo.toml | sed s/"dyn-clonable"/"dyn-clone"/)" > ./polkadot-sdk/substrate/primitives/core/Cargo.toml
apply_patch removals/dyn-clonable

# Remove memmap2 and the associated unsafe calling code
apply_patch removals/memmap2

# Apply the patches which are explicitly opinions
apply_patch_dir opinions

# Now, set up the Rust binary and make all the invasive changes
silent_rm ./target/release/serai-polkadot-sdk # Ensure we aren't using a cached binary
cargo build --release

function remove_crate_tree {
  echo "Removing crates $1"
  ./target/release/serai-polkadot-sdk remove_crate_tree $1
  if [ $? -ne 0 ]; then
    exit 3
  fi
}

function remove_workspace_dependency {
  echo "Removing workspace dependency $1"
  ./target/release/serai-polkadot-sdk remove_workspace_dependency $1
  if [ $? -ne 0 ]; then
    exit 4
  fi
}

function remove_dependency {
  echo "Removing feature $1"
  ./target/release/serai-polkadot-sdk remove_dependency $1
  if [ $? -ne 0 ]; then
    exit 5
  fi
}

function remove_feature {
  echo "Removing feature $1"
  ./target/release/serai-polkadot-sdk remove_feature $1
  if [ $? -ne 0 ]; then
    exit 6
  fi
}

function remove_dev_dependencies {
  ./target/release/serai-polkadot-sdk remove_dev_dependencies
  if [ $? -ne 0 ]; then
    exit 7
  fi
}

function cargo_upgrade {
  echo "Upgrading $1 to $2"
  ./target/release/serai-polkadot-sdk upgrade $1 "$2"
  if [ $? -ne 0 ]; then
    exit 8
  fi
}

function trim_workspace_dependencies {
  ./target/release/serai-polkadot-sdk trim_workspace_dependencies
  if [ $? -ne 0 ]; then
    exit 9
  fi
}

# Remove the `bridges/` tree, as we won't use it
remove_crate_tree bridges
# Cleanup "snowbridge" dependency not removed with `bridges/`
remove_workspace_dependency milagro-bls

# Remove the `cumulus/` tree, intended for parachains
remove_crate_tree cumulus

# Remove the `polkadot/` tree, intended for Polkadot
remove_crate_tree polkadot

# Remove the `docs` crate, which won't compile after this and isn't worth the effort to patch
remove_crate_tree docs

# Remove the `umbrella` crate, which we don't use, so we don't have to
# re-generate it (requiring multiple bespoke binary tools be added to the system
# running this script)
remove_crate_tree umbrella

# Remove the `templates` folder (effectively examples)
remove_crate_tree templates
# And the actual examples
remove_crate_tree substrate/frame/examples
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/examples

# Remove the deprecated crates
remove_crate_tree substrate/deprecated

# Remove the provided binaries, which we don't use
remove_crate_tree substrate/bin

# Remove the unused "bitswap" protocol
# https://github.com/libp2p/rust-libp2p/issues/2632
silent_rm ./polkadot-sdk/substrate/client/network/build.rs
silent_rm ./polkadot-sdk/substrate/client/network/src/bitswap
silent_rm ./polkadot-sdk/substrate/client/network/src/litep2p/shim/bitswap.rs
silent_rm ./polkadot-sdk/substrate/client/network/src/schema/bitswap.v1.2.0.proto
remove_matching_lines ./polkadot-sdk/substrate/client/network/src/lib.rs "mod bitswap;$"
apply_patch removals/bitswap

# Remove the BEEFY consensus crates
remove_crate_tree substrate/client/consensus/beefy
remove_crate_tree substrate/client/merkle-mountain-range
remove_crate_tree substrate/frame/beefy
remove_crate_tree substrate/frame/beefy-mmr
remove_crate_tree substrate/frame/merkle-mountain-range
remove_crate_tree substrate/primitives/consensus/beefy
remove_crate_tree substrate/primitives/merkle-mountain-range

# Remove the SASSAFRAS consensus crates
remove_crate_tree substrate/frame/sassafras
remove_crate_tree substrate/primitives/consensus/sassafras

# Remove the proof-of-work consensus crates
remove_crate_tree substrate/client/consensus/pow
remove_crate_tree substrate/primitives/consensus/pow

# Remove the manual-seal consensus crate
remove_crate_tree substrate/client/consensus/manual-seal

# Remove the Aura consensus crates
remove_crate_tree substrate/client/consensus/aura
remove_crate_tree substrate/frame/aura
remove_crate_tree substrate/primitives/consensus/aura
apply_patch removals/aura

remove_crate_tree substrate/frame/contracts
remove_crate_tree substrate/frame/revive

# Remove the mixnet code
remove_crate_tree substrate/client/mixnet
remove_crate_tree substrate/frame/mixnet
remove_crate_tree substrate/primitives/mixnet
silent_rm ./polkadot-sdk/substrate/client/rpc-api/src/mixnet
remove_matching_lines ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs "mod mixnet;$"
silent_rm ./polkadot-sdk/substrate/client/rpc/src/mixnet
remove_matching_lines ./polkadot-sdk/substrate/client/rpc/src/lib.rs "mod mixnet;$"
silent_rm ./polkadot-sdk/substrate/client/cli/src/params/mixnet_params.rs
remove_matching_lines ./polkadot-sdk/substrate/client/cli/src/params/mod.rs "mod mixnet_params;$"
sed -e s/" mixnet_params::\*,"//g -i ./polkadot-sdk/substrate/client/cli/src/params/mod.rs

# Remove the 'statement store'
remove_crate_tree substrate/client/network/statement
remove_crate_tree substrate/client/statement-store
remove_crate_tree substrate/frame/statement
remove_crate_tree substrate/primitives/statement-store
silent_rm ./polkadot-sdk/substrate/client/rpc-api/src/statement
remove_matching_lines ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs "mod statement;$"
silent_rm ./polkadot-sdk/substrate/client/rpc/src/statement
remove_matching_lines ./polkadot-sdk/substrate/client/rpc/src/lib.rs "mod statement;$"

# Remove the binary Merkle tree code, as we only use the standard base-16 trie
remove_crate_tree substrate/utils/binary-merkle-tree
silent_rm ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/base2.rs
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/mod.rs "mod base2;$"
apply_patch removals/BinaryMerkleTreeProver

# Remove the transaction storage code
remove_crate_tree substrate/frame/transaction-storage
remove_crate_tree substrate/primitives/transaction-storage-proof
apply_patch removals/sp-transaction-storage-proof

# Remove non-Ristretto cryptography
remove_crate_tree substrate/primitives/crypto/ec-utils
remove_feature bls-experimental
silent_rm ./polkadot-sdk/substrate/primitives/core/src/bls.rs
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/bls381.rs
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/ecdsa_bls381.rs
remove_feature bandersnatch-experimental
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/bandersnatch.rs
silent_rm ./polkadot-sdk/substrate/primitives/core/src/bandersnatch.rs
silent_rm ./polkadot-sdk/substrate/primitives/keyring/src/bandersnatch.rs
silent_rm ./polkadot-sdk/substrate/primitives/core/src/paired_crypto.rs
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/src/lib.rs "mod paired_crypto;$"

silent_rm ./polkadot-sdk/substrate/client/cli/src/commands/vanity.rs
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/ecdsa.rs
silent_rm ./polkadot-sdk/substrate/primitives/core/src/ecdsa.rs
silent_rm ./polkadot-sdk/substrate/primitives/keyring/src/ecdsa.rs
silent_rm ./polkadot-sdk/substrate/frame/support/src/crypto/ecdsa.rs
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/ed25519.rs
silent_rm ./polkadot-sdk/substrate/primitives/core/src/ed25519.rs
silent_rm ./polkadot-sdk/substrate/primitives/keyring/src/ed25519.rs
apply_patch removals/ecdsa_ed25519

# Remove metadata

remove_crate_tree substrate/primitives/metadata-ir
remove_crate_tree substrate/frame/metadata-hash-extension

silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand/metadata.rs
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/deprecation.rs
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand/constants.rs
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand/doc_only.rs
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand/documentation.rs
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/parse/extra_constants.rs
silent_rm ./polkadot-sdk/substrate/primitives/api/proc-macro/src/runtime_metadata.rs
remove_feature no-metadata-docs

silent_rm ./polkadot-sdk/substrate/utils/wasm-builder/src/metadata_hash.rs
remove_feature metadata-hash

remove_dependency scale-info

# Add `sp-api` as a direct dependency to `frame-support`, as it sets features on it
echo '[dependencies.sp-api]' >> ./polkadot-sdk/substrate/frame/support/Cargo.toml
echo 'workspace = true' >> ./polkadot-sdk/substrate/frame/support/Cargo.toml
echo 'default-features = false' >> ./polkadot-sdk/substrate/frame/support/Cargo.toml

apply_patch_dir metadata

# Remove the unused `sc-offchain`
remove_crate_tree substrate/client/offchain

# Remove the deprecated native executor
apply_patch removals/NativeExecutor

# Remove unused pallets
remove_crate_tree substrate/frame/alliance
remove_crate_tree substrate/frame/asset-conversion
remove_crate_tree substrate/frame/asset-rate
remove_crate_tree substrate/frame/asset-rewards
remove_crate_tree substrate/frame/assets
remove_crate_tree substrate/frame/assets-freezer
remove_crate_tree substrate/frame/assets-holder
remove_crate_tree substrate/frame/atomic-swap
remove_crate_tree substrate/frame/bags-list
remove_crate_tree substrate/frame/balances
remove_crate_tree substrate/frame/benchmarking/pov
remove_crate_tree substrate/frame/bounties
remove_crate_tree substrate/frame/broker
remove_crate_tree substrate/frame/child-bounties
remove_crate_tree substrate/frame/collective
remove_crate_tree substrate/frame/conviction-voting
remove_crate_tree substrate/frame/core-fellowship
remove_crate_tree substrate/frame/delegated-staking
remove_crate_tree substrate/frame/democracy
remove_crate_tree substrate/frame/derivatives
remove_crate_tree substrate/frame/dummy-dim
remove_crate_tree substrate/frame/elections-phragmen
remove_crate_tree substrate/frame/election-provider-multi-phase
remove_crate_tree substrate/frame/election-provider-support && remove_crate_tree substrate/primitives/npos-elections
remove_crate_tree substrate/frame/fast-unstake
remove_crate_tree substrate/frame/identity
remove_crate_tree substrate/frame/im-online
remove_crate_tree substrate/frame/indices
remove_crate_tree substrate/frame/insecure-randomness-collective-flip
remove_crate_tree substrate/frame/lottery
remove_crate_tree substrate/frame/membership
remove_crate_tree substrate/frame/message-queue
remove_crate_tree substrate/frame/meta-tx
remove_crate_tree substrate/frame/multisig
remove_crate_tree substrate/frame/nft-fractionalization
remove_crate_tree substrate/frame/nfts
remove_crate_tree substrate/frame/nis
remove_crate_tree substrate/frame/node-authorization
remove_crate_tree substrate/frame/nomination-pools
remove_crate_tree substrate/frame/offences
remove_crate_tree substrate/frame/paged-list
remove_crate_tree substrate/frame/parameters
remove_crate_tree substrate/frame/preimage
remove_crate_tree substrate/frame/proxy
remove_crate_tree substrate/frame/ranked-collective
remove_crate_tree substrate/frame/recovery
remove_crate_tree substrate/frame/referenda
remove_crate_tree substrate/frame/remark
remove_crate_tree substrate/frame/root-offences
remove_crate_tree substrate/frame/root-testing
remove_crate_tree substrate/frame/safe-mode
remove_crate_tree substrate/frame/salary
remove_crate_tree substrate/frame/scheduler
remove_crate_tree substrate/frame/scored-pool
remove_crate_tree substrate/frame/society
remove_crate_tree substrate/frame/staking
remove_crate_tree substrate/frame/state-trie-migration
remove_crate_tree substrate/frame/sudo
remove_crate_tree substrate/frame/tips
remove_crate_tree substrate/frame/transaction-payment/asset-conversion-tx-payment
remove_crate_tree substrate/frame/transaction-payment/asset-tx-payment
remove_crate_tree substrate/frame/treasury
remove_crate_tree substrate/frame/tx-pause
remove_crate_tree substrate/frame/uniques
remove_crate_tree substrate/frame/whitelist
remove_crate_tree substrate/frame/verify-signature
remove_crate_tree substrate/frame/vesting
remove_crate_tree substrate/frame/utility

# Remove the scripts used for testing
remove_crate_tree substrate/scripts

# Remove unused utilities
remove_crate_tree substrate/client/runtime-utilities
remove_crate_tree substrate/utils/build-script-utils
remove_crate_tree substrate/utils/frame
remove_crate_tree substrate/utils/substrate-bip39
apply_patch removals/substrate-bip39

# Remove fuzzers
remove_crate_tree substrate/primitives/arithmetic/fuzzer
remove_crate_tree substrate/primitives/core/fuzz
remove_crate_tree substrate/primitives/state-machine/fuzz

# Remove benchmarking code we don't use
remove_crate_tree substrate/frame/session/benchmarking
remove_crate_tree substrate/frame/system/benchmarking
apply_patch removals/frame-system-benchmarking

# Remove all dev dependencies, tests, benches, etc.
remove_dev_dependencies
remove_crate_tree substrate/client/executor/runtime-test
remove_crate_tree substrate/test-utils
remove_crate_tree substrate/primitives/runtime-interface/test-wasm
remove_crate_tree substrate/primitives/runtime-interface/test-wasm-deprecated
remove_crate_tree substrate/primitives/test-primitives

# Remove `aquamarine`, `docify` from the code
# This is done last as it's quite slow, so it's best to do after we've achieved a small tree
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "aquamarine"); echo "$STRIPPED" > {}' \;
apply_patch removals/docify
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "docify"); echo "$STRIPPED" > {}' \;

# Remove the `SS58prefix` constant
apply_patch removals/SS58Prefix
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "SS58Prefix"); echo "$STRIPPED" > {}' \;

# Remove non-ASCII characters
# Remove Unicode characters from the start of strings
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed -E "s/\"([ ]*[^\x00-\x7Fµ][ ]*)+/\"/"); echo "$STRIPPED" > {}' \;
# Remove Unicode characters from the end of strings
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed -E "s/([ ]*[^\x00-\x7Fµ][ ]*)+\"/\"/"); echo "$STRIPPED" > {}' \;

# Remove "; qed"
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed "s/\; qed//"); echo "$STRIPPED" > {}' \;

# Perform upgrades to preferred versions
cargo_upgrade array-bytes 7.0.0
cargo_upgrade async-channel 2.0.0
cargo_upgrade asynchronous-codec 0.7.0
cargo_upgrade cfg-expr 0.20.0
cargo_upgrade console 0.16.0
cargo_upgrade derive_more 1.0.0
cargo_upgrade directories 6.0.0
cargo_upgrade fs4 0.13.0
cargo_upgrade governor 0.10.0
cargo_upgrade hex-literal 1.0.0
cargo_upgrade itertools 0.14.0
cargo_upgrade kvdb-rocksdb 0.20.0
cargo_upgrade libp2p ">= 0.54, <= 0.55"
cargo_upgrade libp2p-kad ">= 0.46, <= 0.47"
cargo_upgrade macro_magic 0.6.0
cargo_upgrade parity-db 0.5.0
cargo_upgrade partial_sort 1.0.0
cargo_upgrade prometheus 0.14.0
cargo_upgrade prost 0.14.0
cargo_upgrade prost-build 0.14.0
cargo_upgrade rustc-hash 2.0.0
cargo_upgrade rustix 1.0.0
cargo_upgrade strum 0.27.0
cargo_upgrade thiserror 2.0.0
cargo_upgrade twox-hash 2.0.0
cargo_upgrade unsigned-varint 0.8.0
cargo_upgrade wasmtime 36.0.0
cargo_upgrade zstd 0.13.0

cd ./polkadot-sdk

# Remove misc unused files
silent_rm .cargo
silent_rm .config
silent_rm .forklift
silent_rm .github
silent_rm .gitlab
silent_rm docker
silent_rm prdoc
silent_rm scripts
silent_rm substrate/.maintain
silent_rm substrate/docker
silent_rm substrate/docs
silent_rm substrate/scripts
silent_rm substrate/zombienet
silent_rm substrate/.dockerignore
silent_rm substrate/.editorconfig
silent_rm substrate/.git-blame-ignore-revs
silent_rm substrate/.gitattributes
silent_rm substrate/README.md
silent_rm .gitignore
silent_rm .gitlab-ci.yml
silent_rm .prdoc.toml
silent_rm .rustfmt.toml
silent_rm CODE_OF_CONDUCT.md
silent_rm CONTRIBUTING.md
silent_rm Plan.toml
silent_rm README.md

# Restore the committed `Cargo.lock` so this is deterministic, if one exists
silent_rm ./Cargo.lock
cp ../Cargo.lock.polkadot-sdk ./Cargo.lock

# Ensure this worked as expected
echo "Running \`cargo check\`"
cargo check --all-features
if [ $? -ne 0 ]; then
  echo "Patched \`polkadot-sdk\` failed to compile"
  exit 10
fi

# Save >10 GB on what should be a static directory of no further use
cargo clean

touch .patched

cd ..

# Remove unused dependencies from the workspace `Cargo.toml`
trim_workspace_dependencies

# Synchronoize the `polkadot-sdk` `Cargo.lock`
silent_rm ./Cargo.lock.polkadot-sdk
cp ./polkadot-sdk/Cargo.lock ./Cargo.lock.polkadot-sdk

echo "Patched"
