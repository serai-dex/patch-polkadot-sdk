function silent_rm {
  $(rm -rf $1)
  return 0
}

# Start by checking out the desired version of the polkadot-sdk

POLKADOT_SDK_COMMIT=2caeef482a437414c6bed2395a16abe08fccbfbb

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
  git init --quiet
  git remote add origin https://github.com/paritytech/polkadot-sdk
  cd ..
fi

cd ./polkadot-sdk
# Ensure we're starting from the intended commit
rm -rf ./substrate
git checkout -f $POLKADOT_SDK_COMMIT &> /dev/null
if [ $? -ne 0 ]; then
  # Try to fetch the commit
  echo "Fetching $POLKADOT_SDK_COMMIT"
  git fetch --depth 1 origin $POLKADOT_SDK_COMMIT --quiet
  # Try again to check it out
  git checkout -f $POLKADOT_SDK_COMMIT --quiet
  if [ $? -ne 0 ]; then
    echo "Failed to checkout $POLKADOT_SDK_COMMIT"
    exit 2
  fi
fi
# Remove the existing `.patched` marker
silent_rm .patched
cd ..

echo "Starting to patch..."

# Our binary, when it makes its modifications, will overwrite all existing
# `Cargo.toml` files until they're unrecognizable. We start by making changes
# _to_ the `Cargo.toml` files accordingly

function apply_patches {
  find ./patches -iname "*.patch" | sort | while read -r patch; do
    echo "Applying patch $patch"
    cd ./polkadot-sdk
    git apply .$patch
    PATCH_SUCCEEDED=$?
    cd ..
    if [ $PATCH_SUCCEEDED -ne 0 ]; then
      exit 3
    fi
  done

  PATCHES_SUCCEEDED=$?
  if [ $PATCHES_SUCCEEDED -ne 0 ]; then
    exit $PATCHES_SUCCEEDED
  fi
}

function remove_matching_lines {
  if [ ! -f $1 ]; then
    return
  fi
  ORIGINAL=$(cat $1)
  STRIPPED=$(echo "$ORIGINAL" | grep -E -v "$2")
  echo "$STRIPPED" > $1
}

function remove_module {
  silent_rm $1/$2.rs
  silent_rm $1/$2
  remove_matching_lines $1/mod.rs "mod $2"
  remove_matching_lines $1/mod.rs "use $2::(.)+;"
  remove_matching_lines $1/lib.rs "mod $2"
  remove_matching_lines $1/lib.rs "use $2::(.)+;"
  remove_matching_lines $1.rs "mod $2"
  remove_matching_lines $1.rs "use $2::(.)+;"
}

function remove_matching_phrase {
  if [ ! -f $1 ]; then
    return
  fi
  ORIGINAL=$(cat $1)
  STRIPPED=$(echo "$ORIGINAL" | sed "s/$2//g")
  echo "$STRIPPED" > $1
}

# Apply `patches/`
apply_patches

# Add `tokio` as a dependency of `sc-telemetry` due to using it in place of `wasm-timer`
echo '[dependencies.tokio]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'version = "1"' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'default-features = false' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'features = ["time"]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml

# Remove unused HTTP module and associated dependencies
remove_module ./polkadot-sdk/substrate/primitives/runtime/src/offchain http
silent_rm ./polkadot-sdk/substrate/client/offchain/src/api/http.rs

# Remove `aquamarine` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "aquamarine"); echo "$STRIPPED" > {}' \;
# Remove `docify` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "docify"); echo "$STRIPPED" > {}' \;
# Remove `simple-mermaid`
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/src/generic/unchecked_extrinsic.rs "simple_mermaid"

# Remove `is-terminal`
find ./polkadot-sdk/substrate/client/tracing -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed s/"is_terminal::IsTerminal"/"std::io::IsTerminal"/); echo "$STRIPPED" > {}' \;

# Remove `wasm-opt` from `substrate-wasm-builder`
remove_matching_lines ./polkadot-sdk/substrate/utils/wasm-builder/Cargo.toml "wasm-opt"

# Remove `sysinfo` from `sc-db`, which is detected as in-use because it's also the name of a `mod`
remove_matching_lines ./polkadot-sdk/substrate/client/db/Cargo.toml "sysinfo"
# Remove `prost-build` from `sc-network`, which is unused yet `machete` doesn't realize
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "prost-build"
# Remove `ed25519_dalek`, `libsecp256k1`, `secp256k1` from `sp-io`
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "ed25519-dalek"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "libsecp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "secp256k1"

# Remove schemars
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/src/weight_v2.rs "schemars"
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/Cargo.toml "schemars"

# Remove the `SessionKeys` trait
remove_module ./polkadot-sdk/substrate/primitives/session/src runtime_api

# Now, set up the Rust binary and make all the invasive changes
silent_rm ./target/release/serai-polkadot-sdk # Ensure we aren't using a cached binary
cargo build --release &> /dev/null
if [ $? -ne 0 ]; then
  echo "Failed to build \`serai-polkadot-sdk\`"
  exit 4
fi

function remove_crate_tree {
  echo "Removing crates $1"
  ./target/release/serai-polkadot-sdk remove_crate_tree $1
  if [ $? -ne 0 ]; then
    exit 5
  fi
}

function remove_dependency {
  echo "Removing feature $1"
  ./target/release/serai-polkadot-sdk remove_dependency $1
  if [ $? -ne 0 ]; then
    exit 6
  fi
}

function remove_feature {
  echo "Removing feature $1"
  ./target/release/serai-polkadot-sdk remove_feature $1
  if [ $? -ne 0 ]; then
    exit 7
  fi
}

function remove_trait {
  echo "Removing trait $2 from $1"
  ./target/release/serai-polkadot-sdk remove_trait $1 $2
  if [ $? -ne 0 ]; then
    exit 8
  fi
}

function remove_dev_dependencies {
  ./target/release/serai-polkadot-sdk remove_dev_dependencies
  if [ $? -ne 0 ]; then
    exit 8
  fi
}

function cargo_upgrade {
  echo "Upgrading $1 to $2"
  ./target/release/serai-polkadot-sdk upgrade $1 "$2"
  if [ $? -ne 0 ]; then
    exit 10
  fi
}

function trim_workspace_dependencies {
  echo "Trimming the workspace \`Cargo.toml\` of unused dependencies"
  ./target/release/serai-polkadot-sdk trim_workspace_dependencies
  if [ $? -ne 0 ]; then
    exit 11
  fi
}

function machete {
  echo "Removing unused dependencies"
  ./target/release/serai-polkadot-sdk machete
  if [ $? -ne 0 ]; then
    exit 12
  fi
}

# Remove the `bridges/` tree, as we won't use it
remove_crate_tree bridges

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

remove_crate_tree substrate/frame/contracts
remove_crate_tree substrate/frame/revive

# Remove the mixnet code
remove_crate_tree substrate/client/mixnet
remove_crate_tree substrate/frame/mixnet
remove_crate_tree substrate/primitives/mixnet
remove_module ./polkadot-sdk/substrate/client/rpc-api/src mixnet
remove_module ./polkadot-sdk/substrate/client/rpc/src mixnet
remove_module ./polkadot-sdk/substrate/client/cli/src/params mixnet_params
sed -e s/" mixnet_params::\*,"//g -i ./polkadot-sdk/substrate/client/cli/src/params/mod.rs

# Remove the 'statement store'
remove_crate_tree substrate/client/network/statement
remove_crate_tree substrate/client/statement-store
remove_crate_tree substrate/frame/statement
remove_crate_tree substrate/primitives/statement-store
remove_module ./polkadot-sdk/substrate/client/rpc-api/src statement
remove_module ./polkadot-sdk/substrate/client/rpc/src statement

# Remove the binary Merkle tree code, as we only use the standard base-16 trie
remove_crate_tree substrate/utils/binary-merkle-tree
remove_module ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie base2

# Remove the transaction storage code
remove_crate_tree substrate/frame/transaction-storage
remove_crate_tree substrate/primitives/transaction-storage-proof

# Remove the `serde_json`-premised genesis handling
remove_module ./polkadot-sdk/substrate/frame/support/src genesis_builder_helper
remove_matching_lines ./polkadot-sdk/substrate/frame/src/lib.rs "genesis_builder_helper"

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
remove_module ./polkadot-sdk/substrate/primitives/core/src ecdsa
silent_rm ./polkadot-sdk/substrate/primitives/keyring/src/ecdsa.rs
remove_module ./polkadot-sdk/substrate/frame/support/src/crypto ecdsa
silent_rm ./polkadot-sdk/substrate/primitives/application-crypto/src/ed25519.rs
remove_module ./polkadot-sdk/substrate/primitives/core/src ed25519
silent_rm ./polkadot-sdk/substrate/primitives/keyring/src/ed25519.rs

# Remove unused pallets
used_pallets="authority-discovery authorship babe benchmarking executive glutton grandpa session support system timestamp try-runtime"
ls ./polkadot-sdk/substrate/frame | sort | while read -r folder; do
  if [ -d ./polkadot-sdk/substrate/frame/$folder ]; then
    if [ $(echo "$used_pallets src" | grep $folder | wc -l) -eq 0 ]; then
      remove_crate_tree substrate/frame/$folder
    fi
  fi
done

# Remove the unused RPC provided for `frame-system`
remove_crate_tree substrate/frame/system/rpc

# Remove unused primitives
remove_crate_tree substrate/primitives/ethereum-standards
remove_crate_tree substrate/primitives/npos-elections

# Remove the scripts used for testing
remove_crate_tree substrate/scripts

# Remove unused utilities
remove_crate_tree substrate/client/runtime-utilities
remove_crate_tree substrate/utils/build-script-utils
remove_crate_tree substrate/utils/frame
remove_crate_tree substrate/utils/substrate-bip39

# Remove fuzzers
remove_crate_tree substrate/primitives/arithmetic/fuzzer
remove_crate_tree substrate/primitives/core/fuzz
remove_crate_tree substrate/primitives/state-machine/fuzz

# Remove benchmarking code we don't use
remove_crate_tree substrate/frame/benchmarking/pov
remove_crate_tree substrate/frame/session/benchmarking
remove_crate_tree substrate/frame/system/benchmarking

# Remove all dev dependencies, tests, benches, etc.
remove_dev_dependencies
remove_crate_tree substrate/client/executor/runtime-test
silent_rm ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/parse/tests
silent_rm ./polkadot-sdk/substrate/primitives/runtime-interface/tests
remove_crate_tree substrate/primitives/runtime-interface/test-wasm
remove_crate_tree substrate/primitives/runtime-interface/test-wasm-deprecated
remove_crate_tree substrate/primitives/test-primitives
remove_crate_tree substrate/test-utils

# Remove metadata

# Remove the metadata hash and associated extension
remove_feature metadata-hash
remove_crate_tree substrate/frame/metadata-hash-extension
silent_rm ./polkadot-sdk/substrate/utils/wasm-builder/src/metadata_hash.rs

# Remove various features for metadata
remove_feature frame-metadata
remove_feature no-metadata-docs
remove_feature full-metadata-docs

# Remove the call metadata trait
remove_trait substrate/frame/support GetCallMetadata

# Remove the runtime's metadata
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand metadata
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand constants
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand doc_only
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand documentation
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/parse extra_constants
remove_module ./polkadot-sdk/substrate/primitives/api/proc-macro/src runtime_metadata

# Remove `sp-metadata-ir`
remove_crate_tree substrate/primitives/metadata-ir
remove_matching_lines ./polkadot-sdk/substrate/frame/support/src/hash.rs "metadata_ir"
ls ./polkadot-sdk/substrate/frame/support/src/storage/types | sort | while read -r file; do
  file=./polkadot-sdk/substrate/frame/support/src/storage/types/$file
  remove_matching_lines $file "^use sp_metadata_ir"
  remove_matching_phrase $file "\, StorageEntryMetadataBuilder"
  remove_matching_phrase $file "StorageEntryMetadataBuilder\, "
done
remove_matching_lines ./polkadot-sdk/substrate/frame/support/src/storage/types/mod.rs "/// Metadata for the storage kind."
remove_matching_lines ./polkadot-sdk/substrate/frame/support/src/storage/types/mod.rs "const METADATA"
remove_trait substrate/frame/support/src/storage/types "StorageEntryMetadataBuilder"
remove_trait substrate/frame/support/procedural/src "InternalConstructRuntime"
remove_matching_lines ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/mod.rs "use #scrate::__private::metadata_ir::InternalImplRuntimeApis;"

# Remove `scale-info`
remove_dependency scale-info
remove_trait substrate TypeInfo

# Remove `use`s of `scale_info`
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -E -v "use scale_info(::(.)+)?;$"); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -E -v "^[[:space:]]scale_info::\{(.)*\},$"); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'STRIPPED=$(cat {} | grep -E -v "((((__private)|(crate))::scale_info)|pallet_prelude)::TypeInfo"); echo "$STRIPPED" > {}' \;

# Remove `(Static)TypeInfo` derivations/bounds
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"\: (scale_info::)?(Static)?TypeInfo,"/,/g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"\: (scale_info::)?(Static)?TypeInfo>"/"\>"/g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"\: (scale_info::)?(Static)?TypeInfo\;"/";"/g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"([ \t])*\+ (scale_info::)?(Static)?TypeInfo"//g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"\, (scale_info::)?(Static)?TypeInfo"//g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"^([ \t])*(scale_info::)?(Static)?TypeInfo,$"//g); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | sed -E s/"\((scale_info::)?(Static)?TypeInfo,([ ])*"/"\("/g); echo "$STRIPPED" > {}' \;

# Remove `#[scale_info(...)]` attributes
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "\#\[scale_info"); echo "$STRIPPED" > {}' \;

# Replace a usage of `scale_info::prelude::hash` which is a reference to `core::hash`
echo "$(cat ./polkadot-sdk/substrate/primitives/core/src/crypto_bytes.rs | sed s/"scale_info::prelude::hash::Hasher"/"core::hash::Hasher"/)" > ./polkadot-sdk/substrate/primitives/core/src/crypto_bytes.rs

# Metadata has now been removed

# Remove `sp-maybe-compressed-blob`
remove_crate_tree substrate/primitives/maybe-compressed-blob

# Remove the unused `sc-offchain`
remove_crate_tree substrate/client/offchain

# Remove `aquamarine`, `docify` from the code
# This is done last as it's quite slow, so it's best to do after we've achieved a small tree
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "aquamarine"); echo "$STRIPPED" > {}' \;
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "docify"); echo "$STRIPPED" > {}' \;

# Remove the `SS58prefix` constant
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | grep -v "SS58Prefix"); echo "$STRIPPED" > {}' \;

# Remove non-ASCII characters
echo "Removing extraneous emojis"
# Remove Unicode characters from the start of strings
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed -E "s/\"([ ]*[^\x00-\x7Fµ][ ]*)+/\"/"); echo "$STRIPPED" > {}' \;
# Remove Unicode characters from the end of strings
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed -E "s/([ ]*[^\x00-\x7Fµ][ ]*)+\"/\"/"); echo "$STRIPPED" > {}' \;

# Remove "; qed"
echo "Removing extraneous \"qed\" claims"
find ./polkadot-sdk/substrate -iname "*.rs" -exec bash -c 'ORIGINAL=$(cat {}); STRIPPED=$(echo "$ORIGINAL" | LC_COLLATE=C sed "s/\; qed//"); echo "$STRIPPED" > {}' \;

# Remove the original sessions module
mv ./polkadot-sdk/substrate/frame/session ./polkadot-sdk/substrate/frame/session-original
cp -r ./patches/opinions/session ./polkadot-sdk/substrate/frame/session
mv ./polkadot-sdk/substrate/frame/session-original ./polkadot-sdk/substrate/frame/session/session
sed -e s/"name = \"pallet-session\""/"name = \"pallet-session-original\""/ -i ./polkadot-sdk/substrate/frame/session/session/Cargo.toml

# Remove unused dependencies
machete

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
cargo_upgrade itertools 0.14.0
cargo_upgrade kvdb-rocksdb 0.20.0
cargo_upgrade libp2p 0.56.0
cargo_upgrade libp2p-kad 0.48.0
cargo_upgrade litep2p 0.10.0
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
cargo_upgrade wasmtime 37.0.0
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
  exit 13
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
