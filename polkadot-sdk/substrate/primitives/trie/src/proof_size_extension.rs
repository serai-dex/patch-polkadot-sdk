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

//! Externalities extension that provides access to the current proof size
//! of the underlying recorder.

use parking_lot::Mutex;

use crate::ProofSizeProvider;
use std::{collections::VecDeque, sync::Arc};

sp_externalities::decl_extension! {
	/// The proof size extension to fetch the current storage proof size
	/// in externalities.
	pub struct ProofSizeExt(Box<dyn ProofSizeProvider + 'static + Sync + Send>);

	impl ProofSizeExt {
		fn start_transaction(&mut self, ty: sp_externalities::TransactionType) {
			self.0.start_transaction(ty.is_host());
		}

		fn rollback_transaction(&mut self, ty: sp_externalities::TransactionType) {
			self.0.rollback_transaction(ty.is_host());
		}

		fn commit_transaction(&mut self, ty: sp_externalities::TransactionType) {
			self.0.commit_transaction(ty.is_host());
		}
	}
}

impl ProofSizeExt {
	/// Creates a new instance of [`ProofSizeExt`].
	pub fn new<T: ProofSizeProvider + Sync + Send + 'static>(recorder: T) -> Self {
		ProofSizeExt(Box::new(recorder))
	}

	/// Returns the storage proof size.
	pub fn storage_proof_size(&self) -> u64 {
		self.0.estimate_encoded_size() as _
	}
}

/// Proof size estimations as recorded by [`RecordingProofSizeProvider`].
///
/// Each item is the estimated proof size as observed when calling
/// [`ProofSizeProvider::estimate_encoded_size`]. The items are ordered by their observation and
/// need to be replayed in the exact same order.
pub struct RecordedProofSizeEstimations(pub VecDeque<usize>);

/// Inner structure of [`RecordingProofSizeProvider`].
struct RecordingProofSizeProviderInner {
	inner: Box<dyn ProofSizeProvider + Send + Sync>,
	/// Stores the observed proof estimations (in order of observation) per transaction.
	///
	/// Last element of the outer vector is the active transaction.
	proof_size_estimations: Vec<Vec<usize>>,
}

/// An implementation of [`ProofSizeProvider`] that records the return value of the calls to
/// [`ProofSizeProvider::estimate_encoded_size`].
///
/// Wraps an inner [`ProofSizeProvider`] that is used to get the actual encoded size estimations.
/// Each estimation is recorded in the order it was observed.
#[derive(Clone)]
pub struct RecordingProofSizeProvider {
	inner: Arc<Mutex<RecordingProofSizeProviderInner>>,
}

impl RecordingProofSizeProvider {
	/// Creates a new instance of [`RecordingProofSizeProvider`].
	pub fn new<T: ProofSizeProvider + Sync + Send + 'static>(recorder: T) -> Self {
		Self {
			inner: Arc::new(Mutex::new(RecordingProofSizeProviderInner {
				inner: Box::new(recorder),
				// Init the always existing transaction.
				proof_size_estimations: vec![Vec::new()],
			})),
		}
	}

	/// Returns the recorded estimations returned by each call to
	/// [`Self::estimate_encoded_size`].
	pub fn recorded_estimations(&self) -> Vec<usize> {
		self.inner.lock().proof_size_estimations.iter().flatten().copied().collect()
	}
}

impl ProofSizeProvider for RecordingProofSizeProvider {
	fn estimate_encoded_size(&self) -> usize {
		let mut inner = self.inner.lock();

		let estimation = inner.inner.estimate_encoded_size();

		inner
			.proof_size_estimations
			.last_mut()
			.expect("There is always at least one transaction open")
			.push(estimation);

		estimation
	}

	fn start_transaction(&mut self, is_host: bool) {
		// We don't care about runtime transactions, because they are part of the consensus critical
		// path, that will always deterministically call this code.
		//
		// For example a runtime execution is creating 10 runtime transaction and calling in every
		// transaction the proof size estimation host function and 8 of these transactions are
		// rolled back. We need to keep all the 10 estimations. When the runtime execution is
		// replayed (by e.g. importing a block), we will deterministically again create 10 runtime
		// executions and roll back 8. However, in between we require all 10 estimations as
		// otherwise the execution would not be deterministically anymore.
		//
		// A host transaction is only rolled back while for example building a block and an
		// extrinsic failed in the early checks in the runtime. In this case, the extrinsic will
		// also never appear in a block and thus, will not need to be replayed later on.
		if is_host {
			self.inner.lock().proof_size_estimations.push(Default::default());
		}
	}

	fn rollback_transaction(&mut self, is_host: bool) {
		let mut inner = self.inner.lock();

		// The host side transaction needs to be reverted, because this is only done when an
		// entire execution is rolled back. So, the execution will never be part of the consensus
		// critical path.
		if is_host && inner.proof_size_estimations.len() > 1 {
			inner.proof_size_estimations.pop();
		}
	}

	fn commit_transaction(&mut self, is_host: bool) {
		let mut inner = self.inner.lock();

		if is_host && inner.proof_size_estimations.len() > 1 {
			let last = inner
				.proof_size_estimations
				.pop()
				.expect("There are more than one element in the vector");

			inner
				.proof_size_estimations
				.last_mut()
				.expect("There are more than one element in the vector")
				.extend(last);
		}
	}
}

/// An implementation of [`ProofSizeProvider`] that replays estimations recorded by
/// [`RecordingProofSizeProvider`].
///
/// The recorded estimations are removed as they are required by calls to
/// [`Self::estimate_encoded_size`]. Will return `0` when all estimations are consumed.
pub struct ReplayProofSizeProvider(Arc<Mutex<RecordedProofSizeEstimations>>);

impl ReplayProofSizeProvider {
	/// Creates a new instance from the given [`RecordedProofSizeEstimations`].
	pub fn from_recorded(recorded: RecordedProofSizeEstimations) -> Self {
		Self(Arc::new(Mutex::new(recorded)))
	}
}

impl From<RecordedProofSizeEstimations> for ReplayProofSizeProvider {
	fn from(value: RecordedProofSizeEstimations) -> Self {
		Self::from_recorded(value)
	}
}

impl ProofSizeProvider for ReplayProofSizeProvider {
	fn estimate_encoded_size(&self) -> usize {
		self.0.lock().0.pop_front().unwrap_or_default()
	}
}

#[cfg(test)]
mod tests {
}
