#!/bin/bash

function silent_rm {
  $(rm -rf $1)
  return 0
}

# Start by checking out the desired version of the polkadot-sdk

POLKADOT_SDK_COMMIT=8e5ee34b9dde1c009a408bb84c47232683b5b390 # stable2509-3

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
  find ./patches -iname "*.patch" | LC_ALL=en_US.UTF8 sort | while read -r patch; do
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
  if [ ! -f "$1" ]; then
    return
  fi
  STRIPPED=$(grep -E -v "$2" "$1")
  echo "$STRIPPED" > "$1"
}

# A regex to match a series of sequential one-line attributes and line comments
MATCH_ATTRIBUTES="([ \t]*((#\[[^\n]*])|((\/\/)[^\n]*))\n)*"

function remove_matching_statement_and_preceding_attributes {
  if [ ! -f $1 ]; then
    return
  fi
  ORIGINAL=$(cat $1)
  LINE_NUMBER=$(echo "$ORIGINAL" | grep -P -n "$2;$" | grep -E -v "^[^:]*:[[:space:]]*///" | head -n1 | cut --delimiter=":" -f1)
  if [ "$LINE_NUMBER" = "" ]; then
    return
  fi
  # Match the attributes, filter empty lines, filter to the first match alone, get its line number
  LINES=$(echo "$ORIGINAL" | grep -Pzo "$MATCH_ATTRIBUTES([^\n])*$2;" | grep --text -Ev "^([[:space:]])*$" | grep --text -n \; | head -n1 | cut --delimiter=":" -f1)
  START_LINE=$(($LINE_NUMBER - ($LINES - 1)))
  echo "$ORIGINAL" | head -n$(($START_LINE - 1)) > $1
  echo "$ORIGINAL" | tail -n+$(($LINE_NUMBER + 1)) >> $1
  remove_matching_statement_and_preceding_attributes $1 "$2"
}

function remove_use {
  if [ ! -f $1 ]; then
    return
  fi

  # Remove the single-line `use` statement for this
  remove_matching_statement_and_preceding_attributes $1 "use $2([^\n;])*"

  # Remove the multi-line `use` statement for this
  ORIGINAL=$(cat $1)
  OPEN_OF_MULTILINE_USE=$(echo "$ORIGINAL" | grep -E -n -m1 "use $2::(.)*{$" | cut --delimiter=":" -f1)
  if [ "$OPEN_OF_MULTILINE_USE" = "" ]; then
    return
  fi
  ATTRIBUTES=$(($(echo "$ORIGINAL" | grep -Pzo "$MATCH_ATTRIBUTES([^\n])*use $2::([^\n])*{" | grep --text -Ev "^([[:space:]])*$" | grep --text -n "use $2::" | head -n1 | cut --delimiter=":" -f1) - 1))
  OPEN_OF_MULTILINE_USE=$(($OPEN_OF_MULTILINE_USE - $ATTRIBUTES))
  LENGTH_OF_MULTILINE_USE=$(echo "$ORIGINAL" | tail -n+$OPEN_OF_MULTILINE_USE | grep -n -m1 "};" | cut --delimiter=":" -f1)
  END_LINE=$(($OPEN_OF_MULTILINE_USE + $LENGTH_OF_MULTILINE_USE))
  echo "$ORIGINAL" | head -n$(($OPEN_OF_MULTILINE_USE - 1)) > $1
  echo "$ORIGINAL" | tail -n+$END_LINE >> $1
}

function remove_module {
  silent_rm $1/$2.rs
  silent_rm $1/$2
  remove_matching_statement_and_preceding_attributes $1/mod.rs "mod $2"
  remove_use $1/mod.rs "$2"
  remove_matching_statement_and_preceding_attributes $1/lib.rs "mod $2"
  remove_use $1/lib.rs "$2"
  remove_matching_statement_and_preceding_attributes $1.rs "mod $2"
  remove_use $1.rs "$2"
}

function remove_matching_phrase {
  if [ ! -f $1 ]; then
    return
  fi
  sed -i s/"$2"//g "$1"
}

# Apply `patches/`
apply_patches

# Add `tokio` as a dependency of `sc-telemetry` due to using it in place of `wasm-timer`
echo '[dependencies.tokio]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'version = "1"' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'default-features = false' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml
echo 'features = ["time"]' >> ./polkadot-sdk/substrate/client/telemetry/Cargo.toml

# Remove `frame-metadata` as a dependency
remove_matching_lines ./polkadot-sdk/substrate/client/tracing/Cargo.toml "frame-metadata"

# Remove `wasm-instrument` as a dependency
remove_matching_lines ./polkadot-sdk/substrate/client/executor/common/Cargo.toml "wasm-instrument"

# Remove unused HTTP module and associated dependencies
remove_module ./polkadot-sdk/substrate/primitives/runtime/src/offchain http
silent_rm ./polkadot-sdk/substrate/client/offchain/src/api/http.rs

# Remove `aquamarine` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'STRIPPED=$(grep -v "aquamarine" "{}"); echo "$STRIPPED" > "{}"' \;
# Remove `docify` from the dependencies
find ./polkadot-sdk/substrate -iname "*.toml" -exec bash -c 'STRIPPED=$(grep -v "docify" "{}"); echo "$STRIPPED" > "{}"' \;
# Remove `simple-mermaid`
remove_matching_lines ./polkadot-sdk/substrate/primitives/runtime/src/generic/unchecked_extrinsic.rs "simple_mermaid"

# Remove `bounded-collections/std`, as it enables all deps underneath it instead of using `?`
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "bounded-collections/std"
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/Cargo.toml "bounded-collections/std"

# Remove `is-terminal`
find ./polkadot-sdk/substrate/client/tracing -iname "*.rs" -exec bash -c 'sed -i s/"is_terminal::IsTerminal"/"std::io::IsTerminal"/ "{}"' \;

# Remove `sysinfo` from `sc-db`, which is detected as in-use because it's also the name of a `mod`
remove_matching_lines ./polkadot-sdk/substrate/client/db/Cargo.toml "sysinfo"
# Remove `prost-build` from `sc-network`, which is unused yet `machete` doesn't realize
remove_matching_lines ./polkadot-sdk/substrate/client/network/Cargo.toml "prost-build"
# Remove `ed25519_dalek`, `libsecp256k1`, `secp256k1` from `sp-io`
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "ed25519-dalek"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "libsecp256k1"
remove_matching_lines ./polkadot-sdk/substrate/primitives/io/Cargo.toml "secp256k1"

# Remove `schemars`
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/src/weight_v2.rs "schemars"
remove_matching_lines ./polkadot-sdk/substrate/primitives/weights/Cargo.toml "schemars"

# Remove the `SessionKeys` trait
remove_module ./polkadot-sdk/substrate/primitives/session/src runtime_api

# Remove various unnecessary features
remove_matching_phrase ./polkadot-sdk/substrate/primitives/consensus/common/Cargo.toml 'features = ["thread-pool"],'
remove_matching_phrase ./polkadot-sdk/substrate/primitives/core/Cargo.toml 'features = ["small_rng"],'

# Remove `polkadot-sdk-frame`, which will end up unused by the end of this
silent_rm ./polkadot-sdk/substrate/frame/src
silent_rm ./polkadot-sdk/substrate/frame/Cargo.toml
remove_matching_lines ./polkadot-sdk/Cargo.toml 'substrate/frame"'

# Remove unused dependencies from the patched `substrate-prometheus-endpoint`
remove_matching_lines ./polkadot-sdk/substrate/utils/prometheus/Cargo.toml "tokio"
remove_matching_lines ./polkadot-sdk/substrate/utils/prometheus/Cargo.toml "\-util"

# Remove usage of the `serde` feature from `sp-staking` which will itself be later removed
sed -i s/'sp-staking = { features = \["serde"\], '/'sp-staking = { '/ ./polkadot-sdk/substrate/frame/babe/Cargo.toml
sed -i s/'sp-staking = { features = \["serde"\], '/'sp-staking = { '/ ./polkadot-sdk/substrate/frame/grandpa/Cargo.toml

# Remove `default-features` from `kvdb-rocksdb`
sed -i s/'kvdb-rocksdb = {'/'kvdb-rocksdb = { default-features = false, '/ ./polkadot-sdk/Cargo.toml

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

# Remove the deprecated crates
remove_crate_tree substrate/deprecated

# Remove the provided binaries, which we don't use
remove_crate_tree substrate/bin

# Remove the unused "bitswap" protocol
# https://github.com/libp2p/rust-libp2p/issues/2632
silent_rm ./polkadot-sdk/substrate/client/network/build.rs
silent_rm ./polkadot-sdk/substrate/client/network/src/litep2p/shim/bitswap.rs
silent_rm ./polkadot-sdk/substrate/client/network/src/schema/bitswap.v1.2.0.proto
remove_module ./polkadot-sdk/substrate/client/network/src bitswap

# Remove the unused light-client networking protocol
remove_crate_tree substrate/client/network/light

# Remove unused consensus crates
remove_crate_tree substrate/client/consensus/manual-seal

remove_crate_tree substrate/client/consensus/aura
remove_crate_tree substrate/primitives/consensus/aura

remove_crate_tree substrate/client/consensus/beefy
remove_crate_tree substrate/client/merkle-mountain-range
remove_crate_tree substrate/primitives/consensus/beefy
remove_crate_tree substrate/primitives/merkle-mountain-range

remove_crate_tree substrate/client/consensus/pow
remove_crate_tree substrate/primitives/consensus/pow

remove_crate_tree substrate/primitives/consensus/sassafras

# Remove the mixnet code
remove_crate_tree substrate/client/mixnet
remove_crate_tree substrate/primitives/mixnet
remove_module ./polkadot-sdk/substrate/client/rpc-api/src mixnet
remove_module ./polkadot-sdk/substrate/client/rpc/src mixnet
remove_module ./polkadot-sdk/substrate/client/cli/src/params mixnet_params
sed -e s/" mixnet_params::\*,"//g -i ./polkadot-sdk/substrate/client/cli/src/params/mod.rs

# Remove the 'statement store'
remove_crate_tree substrate/client/network/statement
remove_crate_tree substrate/client/statement-store
remove_crate_tree substrate/primitives/statement-store
remove_module ./polkadot-sdk/substrate/client/rpc-api/src statement
remove_module ./polkadot-sdk/substrate/client/rpc/src statement

# Remove the binary Merkle tree code, as we only use the standard base-16 trie
remove_crate_tree substrate/utils/binary-merkle-tree
remove_module ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie base2

# Remove the transaction storage code
remove_crate_tree substrate/primitives/transaction-storage-proof

# Remove the `serde_json`-premised genesis handling
remove_module ./polkadot-sdk/substrate/frame/support/src genesis_builder_helper
remove_matching_lines ./polkadot-sdk/substrate/frame/src/lib.rs "genesis_builder_helper"

# Remove non-Ristretto cryptography
remove_feature bandersnatch-experimental
remove_feature bls-experimental

remove_crate_tree substrate/primitives/crypto/ec-utils

remove_module ./polkadot-sdk/substrate/primitives/application-crypto/src ecdsa
remove_module ./polkadot-sdk/substrate/primitives/application-crypto/src ed25519
remove_module ./polkadot-sdk/substrate/primitives/application-crypto/src bandersnatch
remove_module ./polkadot-sdk/substrate/primitives/application-crypto/src bls381
remove_module ./polkadot-sdk/substrate/primitives/application-crypto/src ecdsa_bls381

remove_module ./polkadot-sdk/substrate/primitives/core/src ecdsa
remove_module ./polkadot-sdk/substrate/primitives/core/src ed25519
remove_module ./polkadot-sdk/substrate/primitives/core/src bandersnatch
remove_module ./polkadot-sdk/substrate/primitives/core/src bls
remove_module ./polkadot-sdk/substrate/primitives/core/src paired_crypto

remove_module ./polkadot-sdk/substrate/primitives/keyring/src ecdsa
remove_module ./polkadot-sdk/substrate/primitives/keyring/src ed25519
remove_module ./polkadot-sdk/substrate/primitives/keyring/src bandersnatch

remove_module ./polkadot-sdk/substrate/frame/support/src crypto

remove_module ./polkadot-sdk/substrate/client/cli/src/commands vanity
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "\, vanity::VanityCmd"

# Remove unused pallets
used_pallets="authority-discovery authorship babe benchmarking executive glutton grandpa session support system timestamp try-runtime"
ls ./polkadot-sdk/substrate/frame | while read -r folder; do
  if [ -d ./polkadot-sdk/substrate/frame/$folder ]; then
    if [ $(echo "$used_pallets src" | grep $folder | wc -l) -eq 0 ]; then
      remove_crate_tree substrate/frame/$folder
    fi
  fi
done

# Remove unused RPC code
remove_module ./polkadot-sdk/substrate/client/rpc/src author
remove_module ./polkadot-sdk/substrate/client/rpc/src chain
remove_module ./polkadot-sdk/substrate/client/rpc/src dev
remove_module ./polkadot-sdk/substrate/client/rpc/src offchain
remove_module ./polkadot-sdk/substrate/client/rpc/src state

remove_module ./polkadot-sdk/substrate/client/rpc-api/src author
remove_module ./polkadot-sdk/substrate/client/rpc-api/src chain
remove_module ./polkadot-sdk/substrate/client/rpc-api/src child_state
remove_module ./polkadot-sdk/substrate/client/rpc-api/src dev
remove_module ./polkadot-sdk/substrate/client/rpc-api/src offchain
remove_module ./polkadot-sdk/substrate/client/rpc-api/src state

remove_crate_tree substrate/client/rpc-spec-v2

remove_crate_tree substrate/client/consensus/babe/rpc
remove_crate_tree substrate/client/consensus/grandpa/rpc
remove_crate_tree substrate/client/sync-state-rpc

remove_crate_tree substrate/frame/system/rpc

# Remove unused primitives
remove_crate_tree substrate/primitives/ethereum-standards
remove_crate_tree substrate/primitives/npos-elections

# Remove the scripts used for testing
remove_crate_tree substrate/scripts

# Remove unused utilities
remove_crate_tree substrate/client/runtime-utilities
remove_crate_tree substrate/client/storage-monitor
remove_crate_tree substrate/utils/build-script-utils
remove_crate_tree substrate/utils/frame
remove_crate_tree substrate/utils/substrate-bip39
remove_crate_tree substrate/utils/wasm-builder

# Remove benchmarking code we don't use
remove_crate_tree substrate/frame/benchmarking/pov
remove_crate_tree substrate/frame/session/benchmarking
remove_crate_tree substrate/frame/system/benchmarking

# Remove all dev dependencies, tests, benches, etc.
remove_dev_dependencies
silent_rm ./polkadot-sdk/substrate/client/tracing/src/block/fixtures
function exhaustive_remove {
  find $1 -iname "$2" | while read -r path; do
    # Preserve `testing.rs` when matching `*test*`
    if [ ! $(echo "$path" | grep "testing") = "" ]; then
      continue
    fi
    in_src=$(echo "$path" | grep "src/")
    folder=$(echo "$path" | sed s/"\/[^/]*$"//)
    file=$(echo $path | sed s/"^.*\/"// | sed -e s/"\..*"//)
    fileext=$(echo $path | sed -e s/".*\."//)
    if [ "$in_src" = "" ]; then
      if [ -d "$path" ]; then
        remove_crate_tree $(echo "$path" | sed s/"^\.\/"// | sed s/"polkadot-sdk\/"//)
      else
        silent_rm $path
      fi
    else
      if [ -d "$path" ] || [ $fileext = "rs" ]; then
        remove_module "$folder" "$file"
      fi
      if [ -f "$path" ]; then
        silent_rm $path
      fi
    fi
  done
}
exhaustive_remove ./polkadot-sdk/substrate "*test*"
exhaustive_remove ./polkadot-sdk/substrate "*fixtures*"
exhaustive_remove ./polkadot-sdk/substrate "res"
exhaustive_remove ./polkadot-sdk/substrate "*fuzz*"
exhaustive_remove ./polkadot-sdk/substrate/client "*mock*"
WITHOUT_DOC=$(grep -F -v '#![doc = include_str!("../res/substrate_test' ./polkadot-sdk/substrate/client/chain-spec/src/lib.rs)
echo "$WITHOUT_DOC" > ./polkadot-sdk/substrate/client/chain-spec/src/lib.rs

# Remove "composite"
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand composite_helper
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand lock_id
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand freeze_reason
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand hold_reason
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand slash_reason
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/construct_runtime/expand task
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/parse composite
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/parse tasks
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand composite
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src/pallet/expand tasks
remove_module ./polkadot-sdk/substrate/frame/support/src/traits tasks

# Remove metadata

# Remove various features for metadata
remove_feature metadata-hash
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
ls ./polkadot-sdk/substrate/frame/support/src/storage/types | while read -r file; do
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
grep -E -r -m1 "(scale_info)|(TypeInfo)" ./polkadot-sdk/substrate/ | cut -d ':' -f1 | grep -E "\.rs$" | while IFS= read -r path; do
  file=$(cat "$path")

  # Remove `use`s of `scale_info`
  file=$(echo "$file" | grep -E -v "use scale_info(::(.)+)?;$")
  file=$(echo "$file" | grep -E -v "^[[:space:]]scale_info::\{(.)*\},$")
  file=$(echo "$file" | grep -E -v "((((__private)|(crate))::scale_info)|pallet_prelude)::TypeInfo")

  # Remove `(Static)TypeInfo` derivations/bounds
  file=$(echo "$file" | sed -E s/"[[:space:]]*(\:|\+|\,) (scale_info::)?(Static)?TypeInfo(,|>|\;)?"/"\4"/g)
  # Remove when present as `(TypeInfo`
  file=$(echo "$file" | sed -E s/"\((scale_info::)?(Static)?TypeInfo,[[:space:]]*"/"\("/g)
  # Remove when present on its own line entirely
  file=$(echo "$file" | sed -E s/"^[[:space:]]*(scale_info::)?(Static)?TypeInfo,$"//g)

  # Remove `#[scale_info(...)]` attributes
  file=$(echo "$file" | grep -F -v "#[scale_info")

  echo "$file" > "$path"
done

# Replace a usage of `scale_info::prelude::hash` which is a reference to `core::hash`
sed -i s/"scale_info::prelude::hash::Hasher"/"core::hash::Hasher"/ ./polkadot-sdk/substrate/primitives/core/src/crypto_bytes.rs

# Metadata has now been removed

# Remove `sp-maybe-compressed-blob`
remove_crate_tree substrate/primitives/maybe-compressed-blob

# Remove the unused `sc-offchain`
remove_crate_tree substrate/client/offchain

# Remove various unused primitives
remove_module ./polkadot-sdk/substrate/primitives/rpc/src list
remove_module ./polkadot-sdk/substrate/primitives/rpc/src number

remove_module ./polkadot-sdk/substrate/primitives/runtime/src/offchain storage_lock
remove_module ./polkadot-sdk/substrate/primitives/runtime/src/proving_trie base16

remove_module ./polkadot-sdk/substrate/primitives/runtime/src type_with_default

remove_module ./polkadot-sdk/substrate/primitives/staking/src currency_to_vote
remove_matching_lines ./polkadot-sdk/substrate/primitives/staking/src/lib.rs "CurrencyToVote"
# Prune `sp-staking` after the `SessionIndex`, `EraIndex` type definitions
staking_lines=$(grep -m1 -n "EraIndex" ./polkadot-sdk/substrate/primitives/staking/src/lib.rs | cut --delimiter=":" -f1)
echo "$(cat ./polkadot-sdk/substrate/primitives/staking/src/lib.rs | head -n$staking_lines)" > ./polkadot-sdk/substrate/primitives/staking/src/lib.rs

remove_module ./polkadot-sdk/substrate/primitives/trie/src recorder_ext
remove_module ./polkadot-sdk/substrate/primitives/version/src embed

# Remove unused parts of `frame-support-procedural`
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src crate_version
remove_module ./polkadot-sdk/substrate/frame/support/procedural/src dummy_part_checker
remove_matching_phrase ./polkadot-sdk/substrate/frame/support/src/lib.rs "\, __generate_dummy_part_checker"

# Remove various unused traits
remove_module ./polkadot-sdk/substrate/frame/support/src/traits filter
remove_module ./polkadot-sdk/substrate/frame/support/src/traits preimages
remove_module ./polkadot-sdk/substrate/frame/support/src/traits proving
remove_module ./polkadot-sdk/substrate/frame/support/src/traits messages
remove_module ./polkadot-sdk/substrate/frame/support/src/traits reality
remove_module ./polkadot-sdk/substrate/frame/support/src/traits schedule
remove_module ./polkadot-sdk/substrate/frame/support/src/traits tokens
remove_module ./polkadot-sdk/substrate/frame/support/src/traits tx_pause
remove_module ./polkadot-sdk/substrate/frame/support/src/traits voting

# Remove unused parts of `frame-system`
remove_module ./polkadot-sdk/substrate/frame/system/src extensions
remove_matching_statement_and_preceding_attributes ./polkadot-sdk/substrate/frame/system/src/lib.rs "ExtensionsWeightInfo([^\n;])*"

remove_module ./polkadot-sdk/substrate/frame/system/src migrations
remove_module ./polkadot-sdk/substrate/frame/system/src mock

# Remove unused commands
remove_module ./polkadot-sdk/substrate/client/cli/src/commands build_spec_cmd
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "build_spec_cmd::BuildSpecCmd\,"

remove_module ./polkadot-sdk/substrate/client/cli/src/commands insert_key
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "insert_key::InsertKeyCmd\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands inspect_key
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "inspect_key::InspectKeyCmd\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands inspect_node_key
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "inspect_node_key::InspectNodeKeyCmd\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands key
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "key::KeySubcommand\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands generate_node_key
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "generate_node_key::GenerateKeyCmdCommon\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands generate
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "generate::GenerateCmd\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands sign
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "sign::SignCmd\,"
remove_module ./polkadot-sdk/substrate/client/cli/src/commands verify
remove_matching_phrase ./polkadot-sdk/substrate/client/cli/src/commands/mod.rs "verify::VerifyCmd\,"

# Remove statements using the `SS58Prefix` constant
echo "Removing \`SS58Prefix\`"
grep -E -r -m1 "SS58Prefix" ./polkadot-sdk/substrate/ | cut -d ':' -f1 | grep -E "\.rs$" | while IFS= read -r path; do
  remove_matching_statement_and_preceding_attributes $path "SS58Prefix([^\n;])*"
done

echo "Removing miscealleneous text elements"
find ./polkadot-sdk/substrate -iname "*.rs" | while read -r path; do
  file=$(cat "$path")

  # Remove `aquamarine`, `docify` from the code
  file=$(echo "$file" | grep -v "aquamarine")
  file=$(echo "$file" | grep -v "docify")

  # Remove Unicode characters from the start, end of strings
  UNICODE_TO_REMOVE="[^\x00-\x7fµ]"
  UNICODE_WITH_SURROUNDING_WHITESPACE="([ ]*$UNICODE_TO_REMOVE[ ]*)+"
  file=$(echo "$file" | LC_COLLATE=C sed -E s/"\"$UNICODE_WITH_SURROUNDING_WHITESPACE"/"\""/)
  file=$(echo "$file" | LC_COLLATE=C sed -E s/"$UNICODE_WITH_SURROUNDING_WHITESPACE\""/"\""/)

  # Remove "; qed"
  file=$(echo "$file" | sed s/"\; qed"//g)

  echo "$file" > "$path"

  # Remove inline `test` `mod`ules
  # This test module has a raw string we can't successfully match against here
  if [ $path = "./polkadot-sdk/substrate/client/chain-spec/src/extension.rs" ]; then
    continue
  fi
  TESTS_MOD_OPEN=$(echo "$file" | grep -E -m1 -n "^mod test(s)? {" | cut --delimiter=":" -f1)
  if [ "$TESTS_MOD_OPEN" = "" ]; then
    continue
  fi
  LENGTH_OF_MOD=$(echo "$file" | tail -n+$TESTS_MOD_OPEN | grep -m1 -n "^}" | cut --delimiter=":" -f1)
  # We implement removal by removing all the lines inside of the module
  echo "$file" | head -n$TESTS_MOD_OPEN > $path
  echo "$file" | tail -n+$(($TESTS_MOD_OPEN + LENGTH_OF_MOD - 1)) >> $path
done

# Remove the sessions module for `ShouldEndSession` alone
ORIGINAL_SESSION=$(cat ./polkadot-sdk/substrate/frame/session/src/lib.rs)
silent_rm ./polkadot-sdk/substrate/frame/session/src
mkdir ./polkadot-sdk/substrate/frame/session/src
echo "$ORIGINAL_SESSION" | head -n $(($(echo "$ORIGINAL_SESSION" | grep -F -m1 -n "//!" | cut --delimiter=":" -f1) - 1)) > ./polkadot-sdk/substrate/frame/session/src/lib.rs
echo "
// This file has been modified since as part of https://github.com/serai-dex/patch-polkadot-sdk.
// Please review it for the exact methodology of the changes, yet the work is offered under the same
// terms as offered above.
" >> ./polkadot-sdk/substrate/frame/session/src/lib.rs
echo "#![no_std]" >> ./polkadot-sdk/substrate/frame/session/src/lib.rs
SHOULD_END_SESSION=$(echo "$ORIGINAL_SESSION" | tail -n+$(($(echo "$ORIGINAL_SESSION" | grep -m1 -n "trait ShouldEndSession" | cut --delimiter=":" -f1))))
SHOULD_END_SESSION=$(echo "$SHOULD_END_SESSION" | head -n$(($(echo "$SHOULD_END_SESSION" | grep -m1 -n "^}$" | cut --delimiter=":" -f1))))
echo "$SHOULD_END_SESSION" >> ./polkadot-sdk/substrate/frame/session/src/lib.rs
echo "
/// Get the current session for Substrate's consensus.
pub trait GetCurrentSessionForSubstrate {
  /// Get the session.
  fn get() -> u32;
}

#[frame_support::pallet]
pub mod pallet {
  use crate::GetCurrentSessionForSubstrate;

  #[pallet::config]
  pub trait Config: frame_system::Config {
    /// The item which tracks the session.
    type Session: GetCurrentSessionForSubstrate;
  }

  #[pallet::pallet]
  pub struct Pallet<T>(_);

  impl<T: Config> Pallet<T> {
    /// The current session index for Substrate's consensus.
    pub fn current_index() -> u32 {
      T::Session::get()
    }
  }
}
pub use pallet::*;
" >> ./polkadot-sdk/substrate/frame/session/src/lib.rs

# Remove sc-chain-spec-derive
remove_crate_tree substrate/client/chain-spec/derive
remove_matching_lines ./polkadot-sdk/substrate/client/chain-spec/src/lib.rs "sc_chain_spec_derive"

# Remove `generate_genesis_config`, which is premised on JSON
remove_module ./polkadot-sdk/substrate/frame/support/src generate_genesis_config

# Remove unused dependencies
machete

# Perform upgrades to preferred versions
cargo_upgrade array-bytes 7.0.0
cargo_upgrade async-channel 2.0.0
cargo_upgrade asynchronous-codec 0.7.0
cargo_upgrade cargo_metadata 0.19.0
cargo_upgrade cfg-expr 0.20.0
cargo_upgrade console 0.16.0
cargo_upgrade derive_more 1.0.0
cargo_upgrade directories 6.0.0
cargo_upgrade fs4 0.13.0
cargo_upgrade governor 0.10.0
cargo_upgrade itertools 0.14.0
cargo_upgrade kvdb-rocksdb 0.21.0
cargo_upgrade libp2p 0.56.0
cargo_upgrade libp2p-kad 0.48.0
cargo_upgrade macro_magic 0.6.0
cargo_upgrade parity-db 0.5.0
cargo_upgrade partial_sort 1.0.0
cargo_upgrade primitive-types 0.14.0
remove_matching_lines ./polkadot-sdk/substrate/primitives/core/Cargo.toml "primitive-types/byteorder"
cargo_upgrade prometheus 0.14.0
cargo_upgrade prost 0.14.0
cargo_upgrade prost-build 0.14.0
cargo_upgrade rustc-hash 2.0.0
cargo_upgrade strum 0.27.0
cargo_upgrade thiserror 2.0.0
cargo_upgrade toml 0.9.0
cargo_upgrade trie-db 0.31.0 # https://github.com/paritytech/polkadot-sdk/pull/10573
cargo_upgrade twox-hash 2.0.0
cargo_upgrade unsigned-varint 0.8.0
cargo_upgrade wasmtime 39.0.0
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
silent_rm substrate/primitives/core/check-features-variants.sh
silent_rm substrate/primitives/keyring/check-features-variants.sh
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

# Run `cargo fix` until no further changes occur
SUBSTRATE_HASH=""
while [ ! "$SUBSTRATE_HASH" = "$(find ./substrate -type f -exec sha256sum \{\} \; | sort)" ]; do
  SUBSTRATE_HASH="$(find ./substrate -type f -exec sha256sum \{\} \; | sort)"

  cargo fix --all-features --allow-dirty

  cd ..
  # Re-run `machete`
  machete
  # Remove unused dependencies from the workspace `Cargo.toml`
  trim_workspace_dependencies
  cd polkadot-sdk
done

echo "Running \`cargo check\` for a final time"
cargo check --all-features
if [ $? -ne 0 ]; then
  echo "Patched, fixed, machete'd \`polkadot-sdk\` failed to compile"
  exit 14
fi

# Save >10 GB on what should be a static directory of no further use
cargo clean

touch .patched

cd ..

# Synchronize the `polkadot-sdk` `Cargo.lock`
silent_rm ./Cargo.lock.polkadot-sdk
cp ./polkadot-sdk/Cargo.lock ./Cargo.lock.polkadot-sdk

echo "Patched"
