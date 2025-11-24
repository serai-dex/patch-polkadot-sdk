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

//! Various utilities that help interfacing with wasm runtime code.

/// Pack a pointer and length into an `u64`.
pub fn pack_ptr_and_len(ptr: u32, len: u32) -> u64 {
	// The static assertions from above are changed into a runtime check.
	#[cfg(all(substrate_runtime, feature = "disable_target_static_assertions"))]
	assert_eq!(4, core::mem::size_of::<usize>());

	(u64::from(len) << 32) | u64::from(ptr)
}

/// Unpacks an `u64` into the pointer and length.
///
/// Runtime API functions return a 64-bit value which encodes a pointer in the least-significant
/// 32-bits and a length in the most-significant 32 bits. This interprets the returned value as a
/// pointer, length tuple.
pub fn unpack_ptr_and_len(val: u64) -> (u32, u32) {
	// The static assertions from above are changed into a runtime check.
	#[cfg(all(substrate_runtime, feature = "disable_target_static_assertions"))]
	assert_eq!(4, core::mem::size_of::<usize>());

	let ptr = (val & (!0u32 as u64)) as u32;
	let len = (val >> 32) as u32;

	(ptr, len)
}

#[cfg(test)]
mod tests {
}
