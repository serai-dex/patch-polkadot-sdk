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

//! Custom panic hook with bug report link
//!
//! This crate provides the [`set`] function, which wraps around [`std::panic::set_hook`] and
//! sets up a panic hook that prints a backtrace and invites the user to open an issue to the
//! given URL.
//!
//! By default, the panic handler aborts the process by calling [`std::process::exit`]. This can
//! temporarily be disabled by using an [`AbortGuard`].

use backtrace::Backtrace;
use regex::Regex;
use std::{
	cell::Cell,
	io::{self, Write},
	marker::PhantomData,
	panic::{self, PanicHookInfo},
	sync::LazyLock,
	thread,
};

thread_local! {
	static ON_PANIC: Cell<OnPanic> = Cell::new(OnPanic::Abort);
}

/// Panic action.
#[derive(Debug, Clone, Copy, PartialEq)]
enum OnPanic {
	/// Abort when panic occurs.
	Abort,
	/// Unwind when panic occurs.
	Unwind,
	/// Always unwind even if someone changes strategy to Abort afterwards.
	NeverAbort,
}

/// Set the panic hook.
///
/// Calls [`std::panic::set_hook`] to set up the panic hook.
///
/// The `bug_url` parameter is an invitation for users to visit that URL to submit a bug report
/// in the case where a panic happens.
pub fn set(bug_url: &str, version: &str) {
	panic::set_hook(Box::new({
		let version = version.to_string();
		let bug_url = bug_url.to_string();
		move |c| panic_hook(c, &bug_url, &version)
	}));
}

macro_rules! ABOUT_PANIC {
	() => {
		"
This is a bug. Please report it at:

	{}
"
	};
}

/// Set aborting flag. Returns previous value of the flag.
fn set_abort(on_panic: OnPanic) -> OnPanic {
	ON_PANIC.with(|val| {
		let prev = val.get();
		match prev {
			OnPanic::Abort | OnPanic::Unwind => val.set(on_panic),
			OnPanic::NeverAbort => (),
		}
		prev
	})
}

/// RAII guard for whether panics in the current thread should unwind or abort.
///
/// Sets a thread-local abort flag on construction and reverts to the previous setting when dropped.
/// Does not implement `Send` on purpose.
///
/// > **Note**: Because we restore the previous value when dropped, you are encouraged to leave
/// > the `AbortGuard` on the stack and let it destroy itself naturally.
pub struct AbortGuard {
	/// Value that was in `ABORT` before we created this guard.
	previous_val: OnPanic,
	/// Marker so that `AbortGuard` doesn't implement `Send`.
	_not_send: PhantomData<std::rc::Rc<()>>,
}

impl AbortGuard {
	/// Create a new guard. While the guard is alive, panics that happen in the current thread will
	/// unwind the stack (unless another guard is created afterwards).
	pub fn force_unwind() -> AbortGuard {
		AbortGuard { previous_val: set_abort(OnPanic::Unwind), _not_send: PhantomData }
	}

	/// Create a new guard. While the guard is alive, panics that happen in the current thread will
	/// abort the process (unless another guard is created afterwards).
	pub fn force_abort() -> AbortGuard {
		AbortGuard { previous_val: set_abort(OnPanic::Abort), _not_send: PhantomData }
	}

	/// Create a new guard. While the guard is alive, panics that happen in the current thread will
	/// **never** abort the process (even if `AbortGuard::force_abort()` guard will be created
	/// afterwards).
	pub fn never_abort() -> AbortGuard {
		AbortGuard { previous_val: set_abort(OnPanic::NeverAbort), _not_send: PhantomData }
	}
}

impl Drop for AbortGuard {
	fn drop(&mut self) {
		set_abort(self.previous_val);
	}
}

// NOTE: When making any changes here make sure to also change this function in `sc-tracing`.
fn strip_control_codes(input: &str) -> std::borrow::Cow<str> {
	static RE: LazyLock<Regex> = LazyLock::new(|| {
		Regex::new(
			r#"(?x)
			\x1b\[[^m]+m|        # VT100 escape codes
			[
			  \x00-\x09\x0B-\x1F # ASCII control codes / Unicode C0 control codes, except \n
			  \x7F               # ASCII delete
			  \u{80}-\u{9F}      # Unicode C1 control codes
			  \u{202A}-\u{202E}  # Unicode left-to-right / right-to-left control characters
			  \u{2066}-\u{2069}  # Same as above
			]
		"#,
		)
		.expect("regex parsing doesn't fail")
	});

	RE.replace_all(input, "")
}

/// Function being called when a panic happens.
fn panic_hook(info: &PanicHookInfo, report_url: &str, version: &str) {
	let location = info.location();
	let file = location.as_ref().map(|l| l.file()).unwrap_or("<unknown>");
	let line = location.as_ref().map(|l| l.line()).unwrap_or(0);

	let msg = match info.payload().downcast_ref::<&'static str>() {
		Some(s) => *s,
		None => match info.payload().downcast_ref::<String>() {
			Some(s) => &s[..],
			None => "Box<Any>",
		},
	};

	let msg = strip_control_codes(msg);

	let thread = thread::current();
	let name = thread.name().unwrap_or("<unnamed>");

	let backtrace = Backtrace::new();

	let mut stderr = io::stderr();

	let _ = writeln!(stderr);
	let _ = writeln!(stderr, "====================");
	let _ = writeln!(stderr);
	let _ = writeln!(stderr, "Version: {}", version);
	let _ = writeln!(stderr);
	let _ = writeln!(stderr, "{:?}", backtrace);
	let _ = writeln!(stderr);
	let _ = writeln!(stderr, "Thread '{}' panicked at '{}', {}:{}", name, msg, file, line);

	let _ = writeln!(stderr, ABOUT_PANIC!(), report_url);
	ON_PANIC.with(|val| {
		if val.get() == OnPanic::Abort {
			::std::process::exit(1);
		}
	})
}

#[cfg(test)]
mod tests {
}
