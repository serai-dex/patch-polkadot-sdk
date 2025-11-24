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

//! Contains the [`crate::defer!`] macro for *deferring* the execution
//! of code until the current scope is dropped.
//! This helps with *always* executing cleanup code.

/// Executes the wrapped closure on drop.
///
/// Should be used together with the [`crate::defer!`] macro.
#[must_use]
pub struct DeferGuard<F: FnOnce()>(pub Option<F>);

impl<F: FnOnce()> DeferGuard<F> {
	/// Creates a new `DeferGuard` with the given closure.
	pub fn new(f: F) -> Self {
		Self(Some(f))
	}
}

impl<F: FnOnce()> Drop for DeferGuard<F> {
	fn drop(&mut self) {
		self.0.take().map(|f| f());
	}
}

/// Executes the given code when the current scope is dropped.
///
/// Multiple calls to [`crate::defer!`] will execute the passed codes in reverse order.
/// This also applies to panic stack unwinding.
///
/// # Example
///
/// ```rust
/// use sp_core::defer;
///
/// let message = std::cell::RefCell::new("".to_string());
/// {
/// 	defer!(
/// 		message.borrow_mut().push_str("world!");
/// 	);
/// 	defer!(
/// 		message.borrow_mut().push_str("Hello ");
/// 	);
/// }
/// assert_eq!(*message.borrow(), "Hello world!");
/// ```
#[macro_export]
macro_rules! defer(
	( $( $code:tt )* ) => {
		let _guard = $crate::defer::DeferGuard(Some(|| { $( $code )* }));
	};
);

#[cfg(test)]
mod test {
}
