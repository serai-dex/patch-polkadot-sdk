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

//! Functionality for reading and storing children hashes from db.

use crate::DbHash;
use codec::{Decode, Encode};
use sp_blockchain;
use sp_database::{Database, Transaction};
use std::hash::Hash;

/// Returns the hashes of the children blocks of the block with `parent_hash`.
pub fn read_children<
	K: Eq + Hash + Clone + Encode + Decode,
	V: Eq + Hash + Clone + Encode + Decode,
>(
	db: &dyn Database<DbHash>,
	column: u32,
	prefix: &[u8],
	parent_hash: K,
) -> sp_blockchain::Result<Vec<V>> {
	let mut buf = prefix.to_vec();
	parent_hash.using_encoded(|s| buf.extend(s));

	let raw_val_opt = db.get(column, &buf[..]);

	let raw_val = match raw_val_opt {
		Some(val) => val,
		None => return Ok(Vec::new()),
	};

	let children: Vec<V> = match Decode::decode(&mut &raw_val[..]) {
		Ok(children) => children,
		Err(_) => return Err(sp_blockchain::Error::Backend("Error decoding children".into())),
	};

	Ok(children)
}

/// Insert the key-value pair (`parent_hash`, `children_hashes`) in the transaction.
/// Any existing value is overwritten upon write.
pub fn write_children<
	K: Eq + Hash + Clone + Encode + Decode,
	V: Eq + Hash + Clone + Encode + Decode,
>(
	tx: &mut Transaction<DbHash>,
	column: u32,
	prefix: &[u8],
	parent_hash: K,
	children_hashes: V,
) {
	let mut key = prefix.to_vec();
	parent_hash.using_encoded(|s| key.extend(s));
	tx.set_from_vec(column, &key[..], children_hashes.encode());
}

/// Prepare transaction to remove the children of `parent_hash`.
pub fn remove_children<K: Eq + Hash + Clone + Encode + Decode>(
	tx: &mut Transaction<DbHash>,
	column: u32,
	prefix: &[u8],
	parent_hash: K,
) {
	let mut key = prefix.to_vec();
	parent_hash.using_encoded(|s| key.extend(s));
	tx.remove(column, &key);
}

#[cfg(test)]
mod tests {
}
