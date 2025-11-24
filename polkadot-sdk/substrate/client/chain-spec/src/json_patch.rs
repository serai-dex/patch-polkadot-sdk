// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! A helper module providing json patching functions.

use serde_json::Value;

/// Recursively merges two JSON objects, `a` and `b`, into a single object.
///
/// If a key exists in both objects, the value from `b` will override the value from `a` (also if
/// value in `b` is `null`).
/// If a key exists only in `b` and not in `a`, it will be added to `a`.
/// No keys will be removed from `a`.
///
/// # Arguments
///
/// * `a` - A mutable reference to the target JSON object to merge into.
/// * `b` - The JSON object to merge with `a`.
pub fn merge(a: &mut Value, b: Value) {
	match (a, b) {
		(Value::Object(a), Value::Object(b)) =>
			for (k, v) in b {
				merge(a.entry(k).or_insert(Value::Null), v);
			},
		(a, b) => *a = b,
	};
}

#[cfg(test)]
mod tests {
}
