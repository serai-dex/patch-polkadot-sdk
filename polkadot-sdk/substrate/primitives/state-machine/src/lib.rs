// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Substrate state machine implementation.

#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod backend;
#[cfg(not(substrate_runtime))]
mod basic;
mod error;
mod ext;
#[cfg(not(substrate_runtime))]
mod in_memory_backend;
pub(crate) mod overlayed_changes;
#[cfg(not(substrate_runtime))]
mod read_only;
mod stats;
#[cfg(feature = "std")]
mod testing;
mod trie_backend;
mod trie_backend_essence;

pub use trie_backend::TrieCacheProvider;

#[cfg(feature = "std")]
pub use std_reexport::*;

#[cfg(feature = "std")]
pub use execution::*;
#[cfg(feature = "std")]
pub use log::{debug, error as log_error, warn};
#[cfg(feature = "std")]
pub use tracing::trace;

/// In no_std we skip logs for state_machine, this macro
/// is a noops.
#[cfg(not(feature = "std"))]
#[macro_export]
macro_rules! warn {
	(target: $target:expr, $message:expr $( , $arg:ident )* $( , )?) => {
		{
			$(
				let _ = &$arg;
			)*
		}
	};
	($message:expr, $( $arg:expr, )*) => {
		{
			$(
				let _ = &$arg;
			)*
		}
	};
}

/// In no_std we skip logs for state_machine, this macro
/// is a noops.
#[cfg(not(feature = "std"))]
#[macro_export]
macro_rules! debug {
	(target: $target:expr, $message:expr $( , $arg:ident )* $( , )?) => {
		{
			$(
				let _ = &$arg;
			)*
		}
	};
}

/// In no_std we skip logs for state_machine, this macro
/// is a noops.
#[cfg(not(feature = "std"))]
#[macro_export]
macro_rules! trace {
	(target: $target:expr, $($arg:tt)+) => {
		()
	};
	($($arg:tt)+) => {
		()
	};
}

/// In no_std we skip logs for state_machine, this macro
/// is a noops.
#[cfg(not(feature = "std"))]
#[macro_export]
macro_rules! log_error {
	(target: $target:expr, $($arg:tt)+) => {
		()
	};
	($($arg:tt)+) => {
		()
	};
}

/// Default error type to use with state machine trie backend.
#[cfg(feature = "std")]
pub type DefaultError = String;
/// Error type to use with state machine trie backend.
#[cfg(not(feature = "std"))]
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct DefaultError;

#[cfg(not(feature = "std"))]
impl core::fmt::Display for DefaultError {
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		write!(f, "DefaultError")
	}
}

pub use crate::{
	backend::{Backend, BackendTransaction, IterArgs, KeysIter, PairsIter, StorageIterator},
	error::{Error, ExecutionError},
	ext::Ext,
	overlayed_changes::{
		ChildStorageCollection, IndexOperation, OffchainChangesCollection,
		OffchainOverlayedChanges, OverlayedChanges, StorageChanges, StorageCollection, StorageKey,
		StorageValue,
	},
	stats::{StateMachineStats, UsageInfo, UsageUnit},
	trie_backend::{TrieBackend, TrieBackendBuilder},
	trie_backend_essence::{Storage, TrieBackendStorage},
};

#[cfg(not(substrate_runtime))]
pub use crate::{
	basic::BasicExternalities,
	in_memory_backend::new_in_mem,
	read_only::{InspectState, ReadOnlyExternalities},
};

#[cfg(feature = "std")]
mod std_reexport {
	pub use crate::{testing::TestExternalities, trie_backend::create_proof_check_backend};
	pub use sp_trie::{
		trie_types::{TrieDBMutV0, TrieDBMutV1},
		CompactProof, DBValue, LayoutV0, LayoutV1, MemoryDB, StorageProof, TrieMut,
	};
}

#[cfg(feature = "std")]
mod execution {
	use crate::backend::AsTrieBackend;

	use super::*;
	use codec::Codec;
	use hash_db::Hasher;
	use smallvec::SmallVec;
	use sp_core::{
		hexdisplay::HexDisplay,
		storage::{ChildInfo, ChildType, PrefixedStorageKey},
		traits::{CallContext, CodeExecutor, RuntimeCode},
	};
	use sp_externalities::Extensions;
	use sp_trie::PrefixedMemoryDB;
	use std::collections::{HashMap, HashSet};

	pub(crate) type CallResult<E> = Result<Vec<u8>, E>;

	/// Default handler of the execution manager.
	pub type DefaultHandler<E> = fn(CallResult<E>, CallResult<E>) -> CallResult<E>;

	/// Trie backend with in-memory storage.
	pub type InMemoryBackend<H> = TrieBackend<PrefixedMemoryDB<H>, H>;

	/// Storage backend trust level.
	#[derive(Debug, Clone)]
	pub enum BackendTrustLevel {
		/// Panics from trusted backends are considered justified, and never caught.
		Trusted,
		/// Panics from untrusted backend are caught and interpreted as runtime error.
		/// Untrusted backend may be missing some parts of the trie, so panics are not considered
		/// fatal.
		Untrusted,
	}

	/// The substrate state machine.
	pub struct StateMachine<'a, B, H, Exec>
	where
		H: Hasher,
		B: Backend<H>,
	{
		backend: &'a B,
		exec: &'a Exec,
		method: &'a str,
		call_data: &'a [u8],
		overlay: &'a mut OverlayedChanges<H>,
		extensions: &'a mut Extensions,
		runtime_code: &'a RuntimeCode<'a>,
		stats: StateMachineStats,
		/// The hash of the block the state machine will be executed on.
		///
		/// Used for logging.
		parent_hash: Option<H::Out>,
		context: CallContext,
	}

	impl<'a, B, H, Exec> Drop for StateMachine<'a, B, H, Exec>
	where
		H: Hasher,
		B: Backend<H>,
	{
		fn drop(&mut self) {
			self.backend.register_overlay_stats(&self.stats);
		}
	}

	impl<'a, B, H, Exec> StateMachine<'a, B, H, Exec>
	where
		H: Hasher,
		H::Out: Ord + 'static + codec::Codec,
		Exec: CodeExecutor + Clone + 'static,
		B: Backend<H>,
	{
		/// Creates new substrate state machine.
		pub fn new(
			backend: &'a B,
			overlay: &'a mut OverlayedChanges<H>,
			exec: &'a Exec,
			method: &'a str,
			call_data: &'a [u8],
			extensions: &'a mut Extensions,
			runtime_code: &'a RuntimeCode,
			context: CallContext,
		) -> Self {
			Self {
				backend,
				exec,
				method,
				call_data,
				extensions,
				overlay,
				runtime_code,
				stats: StateMachineStats::default(),
				parent_hash: None,
				context,
			}
		}

		/// Set the given `parent_hash` as the hash of the parent block.
		///
		/// This will be used for improved logging.
		pub fn set_parent_hash(mut self, parent_hash: H::Out) -> Self {
			self.parent_hash = Some(parent_hash);
			self
		}

		/// Execute a call using the given state backend, overlayed changes, and call executor.
		///
		/// On an error, no prospective changes are written to the overlay.
		///
		/// Note: changes to code will be in place if this call is made again. For running partial
		/// blocks (e.g. a transaction at a time), ensure a different method is used.
		///
		/// Returns the SCALE encoded result of the executed function.
		pub fn execute(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
			self.overlay
				.enter_runtime()
				.expect("StateMachine is never called from the runtime");

			let mut ext = Ext::new(self.overlay, self.backend, Some(self.extensions));

			let ext_id = ext.id;

			trace!(
				target: "state",
				ext_id = %HexDisplay::from(&ext_id.to_le_bytes()),
				method = %self.method,
				parent_hash = %self.parent_hash.map(|h| format!("{:?}", h)).unwrap_or_else(|| String::from("None")),
				input = ?HexDisplay::from(&self.call_data),
				"Call",
			);

			let result = self
				.exec
				.call(&mut ext, self.runtime_code, self.method, self.call_data, self.context)
				.0;

			self.overlay
				.exit_runtime()
				.expect("Runtime is not able to call this function in the overlay");

			trace!(
				target: "state",
				ext_id = %HexDisplay::from(&ext_id.to_le_bytes()),
				?result,
				"Return",
			);

			result.map_err(|e| Box::new(e) as Box<_>)
		}
	}

	/// Prove execution using the given state backend, overlayed changes, and call executor.
	pub fn prove_execution<B, H, Exec>(
		backend: &mut B,
		overlay: &mut OverlayedChanges<H>,
		exec: &Exec,
		method: &str,
		call_data: &[u8],
		runtime_code: &RuntimeCode,
	) -> Result<(Vec<u8>, StorageProof), Box<dyn Error>>
	where
		B: AsTrieBackend<H>,
		H: Hasher,
		H::Out: Ord + 'static + codec::Codec,
		Exec: CodeExecutor + Clone + 'static,
	{
		let trie_backend = backend.as_trie_backend();
		prove_execution_on_trie_backend::<_, _, _>(
			trie_backend,
			overlay,
			exec,
			method,
			call_data,
			runtime_code,
			&mut Default::default(),
		)
	}

	/// Prove execution using the given trie backend, overlayed changes, and call executor.
	/// Produces a state-backend-specific "transaction" which can be used to apply the changes
	/// to the backing store, such as the disk.
	/// Execution proof is the set of all 'touched' storage DBValues from the backend.
	///
	/// On an error, no prospective changes are written to the overlay.
	///
	/// Note: changes to code will be in place if this call is made again. For running partial
	/// blocks (e.g. a transaction at a time), ensure a different method is used.
	pub fn prove_execution_on_trie_backend<S, H, Exec>(
		trie_backend: &TrieBackend<S, H>,
		overlay: &mut OverlayedChanges<H>,
		exec: &Exec,
		method: &str,
		call_data: &[u8],
		runtime_code: &RuntimeCode,
		extensions: &mut Extensions,
	) -> Result<(Vec<u8>, StorageProof), Box<dyn Error>>
	where
		S: trie_backend_essence::TrieBackendStorage<H>,
		H: Hasher,
		H::Out: Ord + 'static + codec::Codec,
		Exec: CodeExecutor + 'static + Clone,
	{
		let proving_backend =
			TrieBackendBuilder::wrap(trie_backend).with_recorder(Default::default()).build();

		let result = StateMachine::<_, H, Exec>::new(
			&proving_backend,
			overlay,
			exec,
			method,
			call_data,
			extensions,
			runtime_code,
			CallContext::Offchain,
		)
		.execute()?;

		let proof = proving_backend
			.extract_proof()
			.expect("A recorder was set and thus, a storage proof can be extracted");

		Ok((result, proof))
	}

	/// Check execution proof, generated by `prove_execution` call.
	pub fn execution_proof_check<H, Exec>(
		root: H::Out,
		proof: StorageProof,
		overlay: &mut OverlayedChanges<H>,
		exec: &Exec,
		method: &str,
		call_data: &[u8],
		runtime_code: &RuntimeCode,
	) -> Result<Vec<u8>, Box<dyn Error>>
	where
		H: Hasher + 'static,
		Exec: CodeExecutor + Clone + 'static,
		H::Out: Ord + 'static + codec::Codec,
	{
		let trie_backend = create_proof_check_backend::<H>(root, proof)?;
		execution_proof_check_on_trie_backend::<_, _>(
			&trie_backend,
			overlay,
			exec,
			method,
			call_data,
			runtime_code,
		)
	}

	/// Check execution proof on proving backend, generated by `prove_execution` call.
	pub fn execution_proof_check_on_trie_backend<H, Exec>(
		trie_backend: &TrieBackend<MemoryDB<H>, H>,
		overlay: &mut OverlayedChanges<H>,
		exec: &Exec,
		method: &str,
		call_data: &[u8],
		runtime_code: &RuntimeCode,
	) -> Result<Vec<u8>, Box<dyn Error>>
	where
		H: Hasher,
		H::Out: Ord + 'static + codec::Codec,
		Exec: CodeExecutor + Clone + 'static,
	{
		StateMachine::<_, H, Exec>::new(
			trie_backend,
			overlay,
			exec,
			method,
			call_data,
			&mut Extensions::default(),
			runtime_code,
			CallContext::Offchain,
		)
		.execute()
	}

	/// Generate storage read proof.
	pub fn prove_read<B, H, I>(backend: B, keys: I) -> Result<StorageProof, Box<dyn Error>>
	where
		B: AsTrieBackend<H>,
		H: Hasher,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let trie_backend = backend.as_trie_backend();
		prove_read_on_trie_backend(trie_backend, keys)
	}

	/// State machine only allows a single level
	/// of child trie.
	pub const MAX_NESTED_TRIE_DEPTH: usize = 2;

	/// Multiple key value state.
	/// States are ordered by root storage key.
	#[derive(PartialEq, Eq, Clone)]
	pub struct KeyValueStates(pub Vec<KeyValueStorageLevel>);

	/// A key value state at any storage level.
	#[derive(PartialEq, Eq, Clone)]
	pub struct KeyValueStorageLevel {
		/// State root of the level, for
		/// top trie it is as an empty byte array.
		pub state_root: Vec<u8>,
		/// Storage of parents, empty for top root or
		/// when exporting (building proof).
		pub parent_storage_keys: Vec<Vec<u8>>,
		/// Pair of key and values from this state.
		pub key_values: Vec<(Vec<u8>, Vec<u8>)>,
	}

	impl<I> From<I> for KeyValueStates
	where
		I: IntoIterator<Item = (Vec<u8>, (Vec<(Vec<u8>, Vec<u8>)>, Vec<Vec<u8>>))>,
	{
		fn from(b: I) -> Self {
			let mut result = Vec::new();
			for (state_root, (key_values, storage_paths)) in b.into_iter() {
				result.push(KeyValueStorageLevel {
					state_root,
					key_values,
					parent_storage_keys: storage_paths,
				})
			}
			KeyValueStates(result)
		}
	}

	impl KeyValueStates {
		/// Return total number of key values in states.
		pub fn len(&self) -> usize {
			self.0.iter().fold(0, |nb, state| nb + state.key_values.len())
		}

		/// Update last keys accessed from this state.
		pub fn update_last_key(
			&self,
			stopped_at: usize,
			last: &mut SmallVec<[Vec<u8>; 2]>,
		) -> bool {
			if stopped_at == 0 || stopped_at > MAX_NESTED_TRIE_DEPTH {
				return false
			}
			match stopped_at {
				1 => {
					let top_last =
						self.0.get(0).and_then(|s| s.key_values.last().map(|kv| kv.0.clone()));
					if let Some(top_last) = top_last {
						match last.len() {
							0 => {
								last.push(top_last);
								return true
							},
							2 => {
								last.pop();
							},
							_ => (),
						}
						// update top trie access.
						last[0] = top_last;
						return true
					} else {
						// No change in top trie accesses.
						// Indicates end of reading of a child trie.
						last.truncate(1);
						return true
					}
				},
				2 => {
					let top_last =
						self.0.get(0).and_then(|s| s.key_values.last().map(|kv| kv.0.clone()));
					let child_last =
						self.0.last().and_then(|s| s.key_values.last().map(|kv| kv.0.clone()));

					if let Some(child_last) = child_last {
						if last.is_empty() {
							if let Some(top_last) = top_last {
								last.push(top_last)
							} else {
								return false
							}
						} else if let Some(top_last) = top_last {
							last[0] = top_last;
						}
						if last.len() == 2 {
							last.pop();
						}
						last.push(child_last);
						return true
					} else {
						// stopped at level 2 so child last is define.
						return false
					}
				},
				_ => (),
			}
			false
		}
	}

	/// Generate range storage read proof, with child tries
	/// content.
	/// A size limit is applied to the proof with the
	/// exception that `start_at` and its following element
	/// are always part of the proof.
	/// If a key different than `start_at` is a child trie root,
	/// the child trie content will be included in the proof.
	pub fn prove_range_read_with_child_with_size<B, H>(
		backend: B,
		size_limit: usize,
		start_at: &[Vec<u8>],
	) -> Result<(StorageProof, u32), Box<dyn Error>>
	where
		B: AsTrieBackend<H>,
		H: Hasher,
		H::Out: Ord + Codec,
	{
		let trie_backend = backend.as_trie_backend();
		prove_range_read_with_child_with_size_on_trie_backend(trie_backend, size_limit, start_at)
	}

	/// Generate range storage read proof, with child tries
	/// content.
	/// See `prove_range_read_with_child_with_size`.
	pub fn prove_range_read_with_child_with_size_on_trie_backend<S, H>(
		trie_backend: &TrieBackend<S, H>,
		size_limit: usize,
		start_at: &[Vec<u8>],
	) -> Result<(StorageProof, u32), Box<dyn Error>>
	where
		S: trie_backend_essence::TrieBackendStorage<H>,
		H: Hasher,
		H::Out: Ord + Codec,
	{
		if start_at.len() > MAX_NESTED_TRIE_DEPTH {
			return Err(Box::new("Invalid start of range."))
		}

		let recorder = sp_trie::recorder::Recorder::default();
		let proving_backend =
			TrieBackendBuilder::wrap(trie_backend).with_recorder(recorder.clone()).build();
		let mut count = 0;

		let mut child_roots = HashSet::new();
		let (mut child_key, mut start_at) = if start_at.len() == 2 {
			let storage_key = start_at.get(0).expect("Checked length.").clone();
			if let Some(state_root) = proving_backend
				.storage(&storage_key)
				.map_err(|e| Box::new(e) as Box<dyn Error>)?
			{
				child_roots.insert(state_root);
			} else {
				return Err(Box::new("Invalid range start child trie key."))
			}

			(Some(storage_key), start_at.get(1).cloned())
		} else {
			(None, start_at.get(0).cloned())
		};

		loop {
			let (child_info, depth) = if let Some(storage_key) = child_key.as_ref() {
				let storage_key = PrefixedStorageKey::new_ref(storage_key);
				(
					Some(match ChildType::from_prefixed_key(storage_key) {
						Some((ChildType::ParentKeyId, storage_key)) =>
							ChildInfo::new_default(storage_key),
						None => return Err(Box::new("Invalid range start child trie key.")),
					}),
					2,
				)
			} else {
				(None, 1)
			};

			let start_at_ref = start_at.as_ref().map(AsRef::as_ref);
			let mut switch_child_key = None;
			let mut iter = proving_backend
				.pairs(IterArgs {
					child_info,
					start_at: start_at_ref,
					start_at_exclusive: true,
					..IterArgs::default()
				})
				.map_err(|e| Box::new(e) as Box<dyn Error>)?;

			while let Some(item) = iter.next() {
				let (key, value) = item.map_err(|e| Box::new(e) as Box<dyn Error>)?;

				if depth < MAX_NESTED_TRIE_DEPTH &&
					sp_core::storage::well_known_keys::is_child_storage_key(key.as_slice())
				{
					count += 1;
					// do not add two child trie with same root
					if !child_roots.contains(value.as_slice()) {
						child_roots.insert(value);
						switch_child_key = Some(key);
						break
					}
				} else if recorder.estimate_encoded_size() <= size_limit {
					count += 1;
				} else {
					break
				}
			}

			let completed = iter.was_complete();

			if switch_child_key.is_none() {
				if depth == 1 {
					break
				} else if completed {
					start_at = child_key.take();
				} else {
					break
				}
			} else {
				child_key = switch_child_key;
				start_at = None;
			}
		}

		let proof = proving_backend
			.extract_proof()
			.expect("A recorder was set and thus, a storage proof can be extracted");
		Ok((proof, count))
	}

	/// Generate range storage read proof.
	pub fn prove_range_read_with_size<B, H>(
		backend: B,
		child_info: Option<&ChildInfo>,
		prefix: Option<&[u8]>,
		size_limit: usize,
		start_at: Option<&[u8]>,
	) -> Result<(StorageProof, u32), Box<dyn Error>>
	where
		B: AsTrieBackend<H>,
		H: Hasher,
		H::Out: Ord + Codec,
	{
		let trie_backend = backend.as_trie_backend();
		prove_range_read_with_size_on_trie_backend(
			trie_backend,
			child_info,
			prefix,
			size_limit,
			start_at,
		)
	}

	/// Generate range storage read proof on an existing trie backend.
	pub fn prove_range_read_with_size_on_trie_backend<S, H>(
		trie_backend: &TrieBackend<S, H>,
		child_info: Option<&ChildInfo>,
		prefix: Option<&[u8]>,
		size_limit: usize,
		start_at: Option<&[u8]>,
	) -> Result<(StorageProof, u32), Box<dyn Error>>
	where
		S: trie_backend_essence::TrieBackendStorage<H>,
		H: Hasher,
		H::Out: Ord + Codec,
	{
		let recorder = sp_trie::recorder::Recorder::default();
		let proving_backend =
			TrieBackendBuilder::wrap(trie_backend).with_recorder(recorder.clone()).build();
		let mut count = 0;
		let iter = proving_backend
			// NOTE: Even though the loop below doesn't use these values
			//       this *must* fetch both the keys and the values so that
			//       the proof is correct.
			.pairs(IterArgs {
				child_info: child_info.cloned(),
				prefix,
				start_at,
				..IterArgs::default()
			})
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;

		for item in iter {
			item.map_err(|e| Box::new(e) as Box<dyn Error>)?;
			if count == 0 || recorder.estimate_encoded_size() <= size_limit {
				count += 1;
			} else {
				break
			}
		}

		let proof = proving_backend
			.extract_proof()
			.expect("A recorder was set and thus, a storage proof can be extracted");
		Ok((proof, count))
	}

	/// Generate child storage read proof.
	pub fn prove_child_read<B, H, I>(
		backend: B,
		child_info: &ChildInfo,
		keys: I,
	) -> Result<StorageProof, Box<dyn Error>>
	where
		B: AsTrieBackend<H>,
		H: Hasher,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let trie_backend = backend.as_trie_backend();
		prove_child_read_on_trie_backend(trie_backend, child_info, keys)
	}

	/// Generate storage read proof on pre-created trie backend.
	pub fn prove_read_on_trie_backend<S, H, I>(
		trie_backend: &TrieBackend<S, H>,
		keys: I,
	) -> Result<StorageProof, Box<dyn Error>>
	where
		S: trie_backend_essence::TrieBackendStorage<H>,
		H: Hasher,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let proving_backend =
			TrieBackendBuilder::wrap(trie_backend).with_recorder(Default::default()).build();
		for key in keys.into_iter() {
			proving_backend
				.storage(key.as_ref())
				.map_err(|e| Box::new(e) as Box<dyn Error>)?;
		}

		Ok(proving_backend
			.extract_proof()
			.expect("A recorder was set and thus, a storage proof can be extracted"))
	}

	/// Generate storage read proof on pre-created trie backend.
	pub fn prove_child_read_on_trie_backend<S, H, I>(
		trie_backend: &TrieBackend<S, H>,
		child_info: &ChildInfo,
		keys: I,
	) -> Result<StorageProof, Box<dyn Error>>
	where
		S: trie_backend_essence::TrieBackendStorage<H>,
		H: Hasher,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let proving_backend =
			TrieBackendBuilder::wrap(trie_backend).with_recorder(Default::default()).build();
		for key in keys.into_iter() {
			proving_backend
				.child_storage(child_info, key.as_ref())
				.map_err(|e| Box::new(e) as Box<dyn Error>)?;
		}

		Ok(proving_backend
			.extract_proof()
			.expect("A recorder was set and thus, a storage proof can be extracted"))
	}

	/// Check storage read proof, generated by `prove_read` call.
	pub fn read_proof_check<H, I>(
		root: H::Out,
		proof: StorageProof,
		keys: I,
	) -> Result<HashMap<Vec<u8>, Option<Vec<u8>>>, Box<dyn Error>>
	where
		H: Hasher + 'static,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let proving_backend = create_proof_check_backend::<H>(root, proof)?;
		let mut result = HashMap::new();
		for key in keys.into_iter() {
			let value = read_proof_check_on_proving_backend(&proving_backend, key.as_ref())?;
			result.insert(key.as_ref().to_vec(), value);
		}
		Ok(result)
	}

	/// Check storage range proof with child trie included, generated by
	/// `prove_range_read_with_child_with_size` call.
	///
	/// Returns key values contents and the depth of the pending state iteration
	/// (0 if completed).
	pub fn read_range_proof_check_with_child<H>(
		root: H::Out,
		proof: StorageProof,
		start_at: &[Vec<u8>],
	) -> Result<(KeyValueStates, usize), Box<dyn Error>>
	where
		H: Hasher + 'static,
		H::Out: Ord + Codec,
	{
		let proving_backend = create_proof_check_backend::<H>(root, proof)?;
		read_range_proof_check_with_child_on_proving_backend(&proving_backend, start_at)
	}

	/// Check child storage range proof, generated by `prove_range_read_with_size` call.
	pub fn read_range_proof_check<H>(
		root: H::Out,
		proof: StorageProof,
		child_info: Option<&ChildInfo>,
		prefix: Option<&[u8]>,
		count: Option<u32>,
		start_at: Option<&[u8]>,
	) -> Result<(Vec<(Vec<u8>, Vec<u8>)>, bool), Box<dyn Error>>
	where
		H: Hasher + 'static,
		H::Out: Ord + Codec,
	{
		let proving_backend = create_proof_check_backend::<H>(root, proof)?;
		read_range_proof_check_on_proving_backend(
			&proving_backend,
			child_info,
			prefix,
			count,
			start_at,
		)
	}

	/// Check child storage read proof, generated by `prove_child_read` call.
	pub fn read_child_proof_check<H, I>(
		root: H::Out,
		proof: StorageProof,
		child_info: &ChildInfo,
		keys: I,
	) -> Result<HashMap<Vec<u8>, Option<Vec<u8>>>, Box<dyn Error>>
	where
		H: Hasher + 'static,
		H::Out: Ord + Codec,
		I: IntoIterator,
		I::Item: AsRef<[u8]>,
	{
		let proving_backend = create_proof_check_backend::<H>(root, proof)?;
		let mut result = HashMap::new();
		for key in keys.into_iter() {
			let value = read_child_proof_check_on_proving_backend(
				&proving_backend,
				child_info,
				key.as_ref(),
			)?;
			result.insert(key.as_ref().to_vec(), value);
		}
		Ok(result)
	}

	/// Check storage read proof on pre-created proving backend.
	pub fn read_proof_check_on_proving_backend<H>(
		proving_backend: &TrieBackend<MemoryDB<H>, H>,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn Error>>
	where
		H: Hasher,
		H::Out: Ord + Codec,
	{
		proving_backend.storage(key).map_err(|e| Box::new(e) as Box<dyn Error>)
	}

	/// Check child storage read proof on pre-created proving backend.
	pub fn read_child_proof_check_on_proving_backend<H>(
		proving_backend: &TrieBackend<MemoryDB<H>, H>,
		child_info: &ChildInfo,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn Error>>
	where
		H: Hasher,
		H::Out: Ord + Codec,
	{
		proving_backend
			.child_storage(child_info, key)
			.map_err(|e| Box::new(e) as Box<dyn Error>)
	}

	/// Check storage range proof on pre-created proving backend.
	///
	/// Returns a vector with the read `key => value` pairs and a `bool` that is set to `true` when
	/// all `key => value` pairs could be read and no more are left.
	pub fn read_range_proof_check_on_proving_backend<H>(
		proving_backend: &TrieBackend<MemoryDB<H>, H>,
		child_info: Option<&ChildInfo>,
		prefix: Option<&[u8]>,
		count: Option<u32>,
		start_at: Option<&[u8]>,
	) -> Result<(Vec<(Vec<u8>, Vec<u8>)>, bool), Box<dyn Error>>
	where
		H: Hasher,
		H::Out: Ord + Codec,
	{
		let mut values = Vec::new();
		let mut iter = proving_backend
			.pairs(IterArgs {
				child_info: child_info.cloned(),
				prefix,
				start_at,
				stop_on_incomplete_database: true,
				..IterArgs::default()
			})
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;

		while let Some(item) = iter.next() {
			let (key, value) = item.map_err(|e| Box::new(e) as Box<dyn Error>)?;
			values.push((key, value));
			if !count.as_ref().map_or(true, |c| (values.len() as u32) < *c) {
				break
			}
		}

		Ok((values, iter.was_complete()))
	}

	/// Check storage range proof on pre-created proving backend.
	///
	/// See `read_range_proof_check_with_child`.
	pub fn read_range_proof_check_with_child_on_proving_backend<H>(
		proving_backend: &TrieBackend<MemoryDB<H>, H>,
		start_at: &[Vec<u8>],
	) -> Result<(KeyValueStates, usize), Box<dyn Error>>
	where
		H: Hasher,
		H::Out: Ord + Codec,
	{
		let mut result = vec![KeyValueStorageLevel {
			state_root: Default::default(),
			key_values: Default::default(),
			parent_storage_keys: Default::default(),
		}];
		if start_at.len() > MAX_NESTED_TRIE_DEPTH {
			return Err(Box::new("Invalid start of range."))
		}

		let mut child_roots = HashSet::new();
		let (mut child_key, mut start_at) = if start_at.len() == 2 {
			let storage_key = start_at.get(0).expect("Checked length.").clone();
			let child_key = if let Some(state_root) = proving_backend
				.storage(&storage_key)
				.map_err(|e| Box::new(e) as Box<dyn Error>)?
			{
				child_roots.insert(state_root.clone());
				Some((storage_key, state_root))
			} else {
				return Err(Box::new("Invalid range start child trie key."))
			};

			(child_key, start_at.get(1).cloned())
		} else {
			(None, start_at.get(0).cloned())
		};

		let completed = loop {
			let (child_info, depth) = if let Some((storage_key, state_root)) = child_key.as_ref() {
				result.push(KeyValueStorageLevel {
					state_root: state_root.clone(),
					key_values: Default::default(),
					parent_storage_keys: Default::default(),
				});

				let storage_key = PrefixedStorageKey::new_ref(storage_key);
				(
					Some(match ChildType::from_prefixed_key(storage_key) {
						Some((ChildType::ParentKeyId, storage_key)) =>
							ChildInfo::new_default(storage_key),
						None => return Err(Box::new("Invalid range start child trie key.")),
					}),
					2,
				)
			} else {
				(None, 1)
			};

			let values = if child_info.is_some() {
				&mut result.last_mut().expect("Added above").key_values
			} else {
				&mut result[0].key_values
			};
			let start_at_ref = start_at.as_ref().map(AsRef::as_ref);
			let mut switch_child_key = None;

			let mut iter = proving_backend
				.pairs(IterArgs {
					child_info,
					start_at: start_at_ref,
					start_at_exclusive: true,
					stop_on_incomplete_database: true,
					..IterArgs::default()
				})
				.map_err(|e| Box::new(e) as Box<dyn Error>)?;

			while let Some(item) = iter.next() {
				let (key, value) = item.map_err(|e| Box::new(e) as Box<dyn Error>)?;
				values.push((key.to_vec(), value.to_vec()));

				if depth < MAX_NESTED_TRIE_DEPTH &&
					sp_core::storage::well_known_keys::is_child_storage_key(key.as_slice())
				{
					// Do not add two chid trie with same root.
					if !child_roots.contains(value.as_slice()) {
						child_roots.insert(value.clone());
						switch_child_key = Some((key, value));
						break
					}
				}
			}

			let completed = iter.was_complete();

			if switch_child_key.is_none() {
				if !completed {
					break depth
				}
				if depth == 1 {
					break 0
				} else {
					start_at = child_key.take().map(|entry| entry.0);
				}
			} else {
				child_key = switch_child_key;
				start_at = None;
			}
		};
		Ok((KeyValueStates(result), completed))
	}
}

#[cfg(test)]
mod tests {
}
