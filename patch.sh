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

function remove_crate_tree {
  echo "Removing crates $1"
  cargo run --release -- remove_crate_tree $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_dependency {
  echo "Removing feature $1"
  cargo run --release -- remove_dependency $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_feature {
  echo "Removing feature $1"
  cargo run --release -- remove_feature $1
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function remove_dev_dependencies {
  cargo run --release -- remove_dev_dependencies
  if [ $? -ne 0 ]; then
    exit 2
  fi
}

function apply_patch {
  cd polkadot-sdk
  git apply ../$1
  PATCH_SUCCEEDED=$?
  cd ..
  if [ $PATCH_SUCCEEDED -ne 0 ]; then
    exit 3
  fi
}

# Apply the `wasmtime` patch BEFORE we re-generate every crate's `Cargo.toml`, due to tweaking features
apply_patch patches/wasmtime.patch
# The same for twox-hash
apply_patch patches/twox-hash.patch
# Remove some unused dependencies
echo "$(cat ./polkadot-sdk/substrate/client/network/Cargo.toml | grep -v 'cid')" > ./polkadot-sdk/substrate/client/network/Cargo.toml
echo "$(cat ./polkadot-sdk/substrate/client/network/Cargo.toml | grep -v 'prost')" > ./polkadot-sdk/substrate/client/network/Cargo.toml
echo "$(cat ./polkadot-sdk/substrate/primitives/core/Cargo.toml | grep -v 'ark-vrf')" > ./polkadot-sdk/substrate/primitives/core/Cargo.toml
echo "$(cat ./polkadot-sdk/substrate/primitives/core/Cargo.toml | grep -v 'w3f-bls')" > ./polkadot-sdk/substrate/primitives/core/Cargo.toml
echo "$(cat ./polkadot-sdk/substrate/primitives/weights/Cargo.toml | grep -v 'schemars')" > ./polkadot-sdk/substrate/primitives/weights/Cargo.toml
echo "$(cat ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml | grep -v 'merkleized-metadata')" > ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml
# Also prune dated workspace dependency specifications we don't need
# This lets us detect which ones are actually worth upgrading
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^alloy-core')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^always-assert')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^ark-')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^bincode')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^bounded-vec')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^cid')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^cmd_lib')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^colored')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^criterion')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^gethostname')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^handlebars')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^jemalloc_pprof')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^landlock')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^merkleized-metadata')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^nix')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^procfs')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^quick_cache')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^rstest')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^schemars')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^seccompiler')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^serde-big-array')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^smoldot')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^subxt')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^sysinfo')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^tikv')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^tokio-tungstenite')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^trie-bench')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^wasmi')" > ./polkadot-sdk/Cargo.toml
echo "$(cat ./polkadot-sdk/Cargo.toml | grep -v '^zombienet')" > ./polkadot-sdk/Cargo.toml

remove_crate_tree bridges
# Cleanup 'snowbridge' dependency not removed with `bridges/`
remove_dependency milagro-bls

remove_crate_tree cumulus

remove_crate_tree polkadot

remove_crate_tree docs/sdk

remove_crate_tree umbrella

remove_crate_tree templates

remove_crate_tree substrate/deprecated

remove_crate_tree substrate/utils/frame/generate-bags/node-runtime
remove_crate_tree substrate/test-utils/cli
remove_crate_tree substrate/bin/node

rm polkadot-sdk/substrate/client/network/build.rs
rm -rf polkadot-sdk/substrate/client/network/src/bitswap
rm polkadot-sdk/substrate/client/network/src/litep2p/shim/bitswap.rs
rm polkadot-sdk/substrate/client/network/src/schema/bitswap.v1.2.0.proto
apply_patch patches/remove_bitswap.rs

remove_crate_tree substrate/client/consensus/beefy
remove_crate_tree substrate/client/merkle-mountain-range
remove_crate_tree substrate/frame/beefy
remove_crate_tree substrate/frame/beefy-mmr
remove_crate_tree substrate/frame/merkle-mountain-range
remove_crate_tree substrate/primitives/consensus/beefy
remove_crate_tree substrate/primitives/merkle-mountain-range

remove_crate_tree substrate/frame/sassafras
remove_crate_tree substrate/primitives/consensus/sassafras

remove_crate_tree substrate/client/consensus/pow
remove_crate_tree substrate/primitives/consensus/pow

remove_crate_tree substrate/frame/contracts
remove_crate_tree substrate/frame/revive

remove_crate_tree substrate/client/mixnet
remove_crate_tree substrate/frame/mixnet
remove_crate_tree substrate/primitives/mixnet
rm -rf ./polkadot-sdk/substrate/client/rpc-api/src/mixnet
echo "$(cat ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs | grep -v "mod mixnet;$")" > ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs
rm -rf ./polkadot-sdk/substrate/client/rpc/src/mixnet
echo "$(cat ./polkadot-sdk/substrate/client/rpc/src/lib.rs | grep -v "mod mixnet;$")" > ./polkadot-sdk/substrate/client/rpc/src/lib.rs
rm ./polkadot-sdk/substrate/client/cli/src/params/mixnet_params.rs
echo "$(cat ./polkadot-sdk/substrate/client/cli/src/params/mod.rs | grep -v "mod mixnet_params;$")" > ./polkadot-sdk/substrate/client/cli/src/params/mod.rs
sed -e s/" mixnet_params::\*,"//g -i ./polkadot-sdk/substrate/client/cli/src/params/mod.rs

remove_crate_tree substrate/client/network/statement
remove_crate_tree substrate/client/statement-store
remove_crate_tree substrate/frame/statement
remove_crate_tree substrate/primitives/statement-store
rm -rf ./polkadot-sdk/substrate/client/rpc-api/src/statement
echo "$(cat ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs | grep -v "mod statement;$")" > ./polkadot-sdk/substrate/client/rpc-api/src/lib.rs
rm -rf ./polkadot-sdk/substrate/client/rpc/src/statement
echo "$(cat ./polkadot-sdk/substrate/client/rpc/src/lib.rs | grep -v "mod statement;$")" > ./polkadot-sdk/substrate/client/rpc/src/lib.rs

remove_crate_tree substrate/utils/binary-merkle-tree
rm ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/base2.rs
echo "$(cat ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/mod.rs | grep -v "mod base2;$")" > ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie/mod.rs
apply_patch patches/remove_binary_merkle_tree_prover.patch

remove_crate_tree substrate/frame/transaction-storage
remove_crate_tree substrate/primitives/transaction-storage-proof
apply_patch patches/remove_sp_transaction_storage_proof.patch

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
echo "$(cat ./polkadot-sdk/substrate/primitives/core/src/lib.rs | grep -v "mod paired_crypto;$")" > ./polkadot-sdk/substrate/primitives/core/src/lib.rs

remove_crate_tree substrate/frame/bags-list/fuzzer
remove_crate_tree substrate/frame/bags-list/remote-tests
remove_crate_tree substrate/frame/election-provider-support/solution-type/fuzzer
remove_crate_tree substrate/frame/paged-list/fuzzer
remove_crate_tree substrate/primitives/arithmetic/fuzzer
remove_crate_tree substrate/primitives/npos-elections/fuzzer

remove_crate_tree substrate/frame/metadata-hash-extension
apply_patch patches/remove_metadata_hash_extension.patch

remove_feature metadata-hash
rm ./polkadot-sdk/substrate/utils/wasm-builder/src/metadata_hash.rs
echo "$(cat ./polkadot-sdk/substrate/test-utils/runtime/build.rs | grep -v 'enable_metadata_hash')" > ./polkadot-sdk/substrate/test-utils/runtime/build.rs

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
remove_crate_tree substrate/frame/examples
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
remove_crate_tree substrate/frame/salary
remove_crate_tree substrate/frame/scored-pool
remove_crate_tree substrate/frame/society
remove_crate_tree substrate/frame/tips
remove_crate_tree substrate/frame/transaction-payment/asset-conversion-tx-payment
remove_crate_tree substrate/frame/transaction-payment/asset-tx-payment
remove_crate_tree substrate/frame/treasury
remove_crate_tree substrate/frame/vesting

remove_crate_tree substrate/scripts

remove_crate_tree substrate/frame/system/benchmarking
apply_patch patches/remove_frame_system_benchmarking.patch

remove_crate_tree substrate/utils/frame/benchmarking-cli
remove_crate_tree substrate/utils/frame/omni-bencher

remove_dev_dependencies

# Remove the `SS58prefix` constant
apply_patch patches/remove_ss58_prefix.patch
find ./polkadot-sdk/substrate -iname "*.rs" -exec sh -c "cat {} | grep -v SS58Prefix > {}.2 && rm {} && mv {}.2 {}" \;

cd polkadot-sdk

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
rm .gitlab-ci.yml
rm .rustfmt.toml
rm Cargo.lock
rm CODE_OF_CONDUCT.md
rm CONTRIBUTING.md
rm README.md
rm .prdoc.toml
rm Plan.toml

# Restore the committed `Cargo.lock` so this is deterministic, if one exists
cp ../Cargo.lock.polkadot-sdk ./Cargo.lock || true

# Ensure this worked as expected
echo "Running \`cargo check\`"
cargo +1.88 check --all-features
if [ $? -ne 0 ]; then
  echo "Patched \`polkadot-sdk\` failed to compile"
  exit 4
fi

# Save >10 GB on what should be a static directory of no further use
cargo clean

touch .patched

cd ..

echo "Patched"

# TODO rm substrate/primitives/core/src/ecdsa.rs
# TODO rm substrate/primitives/application-crypto/src/ecdsa.rs
# TODO rm substrate/primitives/application-crypto/test/src/ecdsa.rs

# TODO rm substrate/primitives/application-crypto/src/ed25519.rs
# TODO rm substrate/primitives/application-crypto/test/src/ed25519.rs
# TODO rm substrate/primitives/core/src/ed25519.rs
# TODO rm substrate/primitives/keyring/src/ed25519.rs

# TODO rm substrate/client/offchain/src/api/http.rs
# TODO rm substrate/frame/support/procedural/src/pallet/expand/constants.rs
# TODO rm substrate/frame/support/procedural/src/pallet/expand/documentation.rs
# TODO rm substrate/frame/support/src/crypto/ecdsa.rs
# TODO rm substrate/frame/support/src/traits/proving.rs
# TODO rm substrate/frame/support/src/traits/voting.rs
# TODO rm substrate/primitives/core/src/paired_crypto.rs
# TODO rm substrate/primitives/metadata-ir/src/unstable.rs
# TODO rm substrate/primitives/metadata-ir/src/v14.rs
# TODO rm substrate/primitives/metadata-ir/src/v15.rs
# TODO rm substrate/primitives/runtime/src/offchain/http.rs
# TODO rm substrate/utils/wasm-builder/src/metadata_hash.rs
