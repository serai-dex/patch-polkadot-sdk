# Start by checking out the desired version of the polkadot-sdk

POLKADOT_SDK_COMMIT=52f4a08f26f226de93c0dbea5e8d066cbbd5bbd0

if [ -f "polkadot-sdk/.patched" ]; then
  if [ ! "$1" = "--from-scratch" ]; then
    echo "\`polkadot-sdk\` was already patched. Run with \`--from-scratch\` to continue"
    exit 1
  fi
fi

# If we're running this script yet `polkadot-sdk` isn't a valid Git repository, clean it
if [ -d "polkadot-sdk" ]; then
  if [ ! -d "polkadot-sdk/.git" ]; then
    rm -rf polkadot-sdk
  fi
fi

# Clone `polkadot-sdk`, yet only the specific commit we're patching
# Ideally, this would be `git clone --revision $POLKADOT_SDK_COMMIT --depth 1`,
# yet that requires a newer Git than frequently packaged
if [ ! -d "polkadot-sdk" ]; then
  mkdir polkadot-sdk
  cd polkadot-sdk
  git init
  git remote add origin https://github.com/paritytech/polkadot-sdk
  git fetch --depth 1 origin $POLKADOT_SDK_COMMIT
  cd ..
fi

cd polkadot-sdk
# Ensure we're starting from the intended commit
git checkout -f $POLKADOT_SDK_COMMIT
# Remove the existing `.patched` marker
rm .patched
cd ..

# Our binary, when it makes its modifications, will overwrite all existing
# `Cargo.toml` files until they're unrecognizable. We start by making changes
# _to_ the `Cargo.toml` files accordingly

function apply_patch {
  cd polkadot-sdk
  git apply ../$1
  PATCH_SUCCEEDED=$?
  cd ..
  if [ $PATCH_SUCCEEDED -ne 0 ]; then
    exit 3
  fi
}

function remove_matching_lines {
  ORIGINAL=$(cat $1)
  STRIPPED=$(echo "$ORIGINAL" | grep -v "$2")
  echo "$STRIPPED" > $1
}

# Apply the `wasmtime` patch, which will update the features within the `Cargo.toml`s
apply_patch patches/wasmtime.patch
# The same for `twox-hash`
apply_patch patches/twox-hash.patch

# Remove some unused dependencies
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "cid"
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "prost"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "ark-vrf"
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "w3f-bls"
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/Cargo.toml "schemars"
remove_matching_lines ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml "merkleized-metadata"
# Also prune dated workspace dependency specifications we don't need
# This lets us detect which ones are actually worth upgrading
remove_matching_lines ./polkadot-sdk/Cargo.toml "^alloy-core"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^always-assert"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^ark-"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^bincode"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^bounded-vec"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^cid"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^cmd_lib"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^colored"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^criterion"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^gethostname"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^handlebars"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^jemalloc_pprof"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^landlock"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^merkleized-metadata"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^nix"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^procfs"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^quick_cache"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^rstest"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^schemars"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^seccompiler"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^serde-big-array"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^smoldot"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^subxt"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^sysinfo"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^tikv"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^tokio-tungstenite"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^trie-bench"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^wasmi"
remove_matching_lines ./polkadot-sdk/Cargo.toml "^zombienet"

# Now, set up the Rust binary and make all the invasive changes
cargo build --release

function remove_crate_tree {
  echo "Removing crates $1"
  ./target/release/serai-polkadot-sdk remove_crate_tree $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_workspace_dependency {
  echo "Removing workspace dependency $1"
  ./target/release/serai-polkadot-sdk remove_workspace_dependency $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_feature {
  echo "Removing feature $1"
  ./target/release/serai-polkadot-sdk remove_feature $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_dev_dependencies {
  ./target/release/serai-polkadot-sdk remove_dev_dependencies
  if [ $? -ne 0 ]; then
    exit 2
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

# Remove the `docs/sdk` crate, which won't compile after this and isn't worth
# the effort to patch
remove_crate_tree docs/sdk

# Remove the `umbrella` crate, which we don't use, so we don't have to
# re-generate it (requiring multiple bespoke binary tools be added to the system
# running this script)
remove_crate_tree umbrella

# Remove the `templates` folder (effectively examples)
remove_crate_tree templates
# And the actual examples
remove_crate_tree substrate/frame/examples

# Remove the deprecated crates
remove_crate_tree substrate/deprecated

# Remove the "kitchensink" runtime, which has everything including the kitchen
# sink, as we remove most things
remove_crate_tree substrate/utils/frame/generate-bags/node-runtime
remove_crate_tree substrate/test-utils/cli
remove_crate_tree substrate/bin/node

# Remove the unused "bitswap" protocol
rm polkadot-sdk/substrate/client/network/build.rs
rm -rf polkadot-sdk/substrate/client/network/src/bitswap
rm polkadot-sdk/substrate/client/network/src/litep2p/shim/bitswap.rs
rm polkadot-sdk/substrate/client/network/src/schema/bitswap.v1.2.0.proto
apply_patch patches/remove_bitswap.rs

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

remove_crate_tree substrate/frame/contracts
remove_crate_tree substrate/frame/revive

# Remove the mixnet code
remove_crate_tree substrate/client/mixnet
remove_crate_tree substrate/frame/mixnet
remove_crate_tree substrate/primitives/mixnet
rm -rf ./polkadot-sdk/substrate/client/rpc-api/src/mixnet
remove_matching_lines ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs "mod mixnet;$"
rm -rf ./polkadot-sdk/substrate/client/rpc/src/mixnet
remove_matching_lines ./polkadot-sdk/substrate/client/rpc/src/lib.rs "mod mixnet;$"
rm ./polkadot-sdk/substrate/client/cli/src/params/mixnet_params.rs
remove_matching_lines ./polkadot-sdk/substrate/client/cli/src/params/mod.rs "mod mixnet_params;$"
sed -e s/" mixnet_params::\*,"//g -i ./polkadot-sdk/substrate/client/cli/src/params/mod.rs

# Remove the 'statement store'
remove_crate_tree substrate/client/network/statement
remove_crate_tree substrate/client/statement-store
remove_crate_tree substrate/frame/statement
remove_crate_tree substrate/primitives/statement-store
rm -rf ./polkadot-sdk/substrate/client/rpc-api/src/statement
remove_matching_lines ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs "mod statement;$"
rm -rf ./polkadot-sdk/substrate/client/rpc/src/statement
remove_matching_lines ./polkadot-sdk/substrate/client/rpc/src/lib.rs "mod statement;$"

# Remove the binary Merkle tree code, as we only use the standard base-16 trie
remove_crate_tree substrate/utils/binary-merkle-tree
rm ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/base2.rs
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/mod.rs "mod base2;$"
apply_patch patches/remove_binary_merkle_tree_prover.patch

# Remove the transaction storage code
remove_crate_tree substrate/frame/transaction-storage
remove_crate_tree substrate/primitives/transaction-storage-proof
apply_patch patches/remove_sp_transaction_storage_proof.patch

# Remove non-Ristretto cryptography
remove_crate_tree substrate/primitives/crypto/ec-utils
remove_feature bls-experimental
rm ./polkadot-sdk/substrate/primitives/core/src/bls.rs
rm ./polkadot-sdk/substrate/primitives/application-crypto/src/bls381.rs
rm ./polkadot-sdk/substrate/primitives/application-crypto/src/ecdsa_bls381.rs
remove_feature bandersnatch-experimental
rm ./polkadot-sdk/substrate/primitives/application-crypto/src/bandersnatch.rs
rm ./polkadot-sdk/substrate/primitives/core/src/bandersnatch.rs
rm ./polkadot-sdk/substrate/primitives/keyring/src/bandersnatch.rs
rm ./polkadot-sdk/substrate/primitives/core/src/paired_crypto.rs
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/src/lib.rs "mod paired_crypto;$"

# TODO rm substrate/primitives/core/src/ecdsa.rs
# TODO rm substrate/primitives/application-crypto/src/ecdsa.rs
# TODO rm substrate/primitives/application-crypto/test/src/ecdsa.rs
# TODO rm substrate/frame/support/src/crypto/ecdsa.rs

# TODO rm substrate/primitives/application-crypto/src/ed25519.rs
# TODO rm substrate/primitives/application-crypto/test/src/ed25519.rs
# TODO rm substrate/primitives/core/src/ed25519.rs
# TODO rm substrate/primitives/keyring/src/ed25519.rs

# Remove the metadata's hash from the runtime
remove_feature metadata-hash
rm ./polkadot-sdk/substrate/utils/wasm-builder/src/metadata_hash.rs
remove_matching_lines ./polkadot-sdk/substrate/test-utils/runtime/build.rs "enable_metadata_hash"

# Remove the extension which checks the metadata's hash from the runtime
remove_crate_tree substrate/frame/metadata-hash-extension
apply_patch patches/remove_metadata_hash_extension.patch

# Remove unused pallets
remove_crate_tree substrate/frame/alliance
remove_crate_tree substrate/frame/asset-conversion
remove_crate_tree substrate/frame/asset-rate
remove_crate_tree substrate/frame/asset-rewards
remove_crate_tree substrate/frame/assets
remove_crate_tree substrate/frame/assets-freezer
remove_crate_tree substrate/frame/assets-holder
remove_crate_tree substrate/frame/atomic-swap
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
remove_crate_tree substrate/frame/fast-unstake
remove_crate_tree substrate/frame/lottery
remove_crate_tree substrate/frame/membership
remove_crate_tree substrate/frame/multisig
remove_crate_tree substrate/frame/nft-fractionalization
remove_crate_tree substrate/frame/nfts
remove_crate_tree substrate/frame/nis
remove_crate_tree substrate/frame/node-authorization
remove_crate_tree substrate/frame/nomination-pools
remove_crate_tree substrate/frame/parameters
remove_crate_tree substrate/frame/ranked-collective
remove_crate_tree substrate/frame/recovery
remove_crate_tree substrate/frame/referenda
remove_crate_tree substrate/frame/root-offences
remove_crate_tree substrate/frame/root-testing
remove_crate_tree substrate/frame/salary
remove_crate_tree substrate/frame/scored-pool
remove_crate_tree substrate/frame/society
remove_crate_tree substrate/frame/sudo
remove_crate_tree substrate/frame/tips
remove_crate_tree substrate/frame/transaction-payment/asset-conversion-tx-payment
remove_crate_tree substrate/frame/transaction-payment/asset-tx-payment
remove_crate_tree substrate/frame/treasury
remove_crate_tree substrate/frame/vesting

# Remove the `scripts` used for testing
remove_crate_tree substrate/scripts

# Remove fuzzers
remove_crate_tree substrate/frame/bags-list/fuzzer
remove_crate_tree substrate/frame/bags-list/remote-tests
remove_crate_tree substrate/frame/election-provider-support/solution-type/fuzzer
remove_crate_tree substrate/frame/paged-list/fuzzer
remove_crate_tree substrate/primitives/arithmetic/fuzzer
remove_crate_tree substrate/primitives/npos-elections/fuzzer
remove_crate_tree substrate/primitives/state-machine/fuzz

# Remove benchmarking code we don't use
remove_crate_tree substrate/frame/election-provider-support/benchmarking
remove_crate_tree substrate/frame/offences/benchmarking
remove_crate_tree substrate/frame/session/benchmarking
remove_crate_tree substrate/frame/system/benchmarking
apply_patch patches/remove_frame_system_benchmarking.patch
remove_crate_tree substrate/utils/frame/benchmarking-cli
remove_crate_tree substrate/utils/frame/omni-bencher

# Remove all dev dependencies, tests, benches, etc.
remove_dev_dependencies

# Remove the `SS58prefix` constant
apply_patch patches/remove_ss58_prefix.patch
find ./polkadot-sdk/substrate -iname "*.rs" -exec sh -c "cat {} | grep -v SS58Prefix > {}.2 && rm {} && mv {}.2 {}" \;

cd polkadot-sdk

# Perform upgrades to preferred versions
cargo +nightly update -Z unstable-options --breaking -p twox-hash --precise 2.1.1
cargo +nightly update -Z unstable-options --breaking -p hex-literal --precise 1.0.0
cargo +nightly update -Z unstable-options --breaking -p thiserror --precise 2.0.16
cargo +nightly update -Z unstable-options --breaking -p itertools --precise 0.14.0
cargo +nightly update -Z unstable-options --breaking -p wasmtime --precise 36.0.2
cargo +nightly update -Z unstable-options --breaking -p rustix --precise 1.0.3
cargo +nightly update -Z unstable-options --breaking -p zstd --precise 0.13.3

# Remove misc unused files
rm -rf .cargo
rm -rf .config
rm -rf .forklift
rm -rf .github
rm -rf .gitlab
rm -rf docker
rm -rf scripts
rm -rf substrate/.maintain
rm -rf substrate/scripts
rm -rf prdoc
rm .gitignore
rm .gitlab-ci.yml
rm .rustfmt.toml
rm Cargo.lock
rm CODE_OF_CONDUCT.md
rm CONTRIBUTING.md
rm README.md
rm .prdoc.toml
rm Plan.toml

# Restore the committed `Cargo.lock` so this is deterministic, if one exists
rm ./Cargo.lock || true
cp ../Cargo.lock.polkadot-sdk ./Cargo.lock || true

# Ensure this worked as expected
echo "Running \`cargo check\`"
cargo check --all-features
if [ $? -ne 0 ]; then
  echo "Patched \`polkadot-sdk\` failed to compile"
  exit 4
fi

# Save >10 GB on what should be a static directory of no further use
cargo clean

touch .patched

cd ..

echo "Patched"

# TODO rm substrate/client/offchain/src/api/http.rs
# TODO rm substrate/primitives/runtime/src/offchain/http.rs
# TODO rm substrate/primitives/metadata-ir/src/unstable.rs
# TODO rm substrate/primitives/metadata-ir/src/v14.rs
# TODO rm substrate/primitives/metadata-ir/src/v15.rs
