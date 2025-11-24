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

//! Types to check that the entire storage can be decoded.

use super::StorageInstance;
use crate::{
	storage::types::{
		CountedStorageMapInstance, CountedStorageNMapInstance, Counter, KeyGenerator,
		QueryKindTrait,
	},
	traits::{PartialStorageInfoTrait, StorageInfo},
	StorageHasher,
};
use alloc::{vec, vec::Vec};
use codec::{Decode, DecodeAll, FullCodec};
use impl_trait_for_tuples::impl_for_tuples;
use sp_core::Get;

/// Decode the entire data under the given storage type.
///
/// For values, this is trivial. For all kinds of maps, it should decode all the values associated
/// with all keys existing in the map.
///
/// Tuple implementations are provided and simply decode each type in the tuple, summing up the
/// decoded bytes if `Ok(_)`.
pub trait TryDecodeEntireStorage {
	/// Decode the entire data under the given storage, returning `Ok(bytes_decoded)` if success.
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>>;
}

#[cfg_attr(all(not(feature = "tuples-96"), not(feature = "tuples-128")), impl_for_tuples(64))]
#[cfg_attr(all(feature = "tuples-96", not(feature = "tuples-128")), impl_for_tuples(96))]
#[cfg_attr(feature = "tuples-128", impl_for_tuples(128))]
impl TryDecodeEntireStorage for Tuple {
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let mut errors = Vec::new();
		let mut len = 0usize;

		for_tuples!(#(
			match Tuple::try_decode_entire_state() {
				Ok(bytes) => len += bytes,
				Err(errs) => errors.extend(errs),
			}
		)*);

		if errors.is_empty() {
			Ok(len)
		} else {
			Err(errors)
		}
	}
}

/// A value could not be decoded.
#[derive(Clone, PartialEq, Eq)]
pub struct TryDecodeEntireStorageError {
	/// The key of the undecodable value.
	pub key: Vec<u8>,
	/// The raw value.
	pub raw: Option<Vec<u8>>,
	/// The storage info of the key.
	pub info: StorageInfo,
}

impl core::fmt::Display for TryDecodeEntireStorageError {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"`{}::{}` key `{}` is undecodable",
			&alloc::str::from_utf8(&self.info.pallet_name).unwrap_or("<invalid>"),
			&alloc::str::from_utf8(&self.info.storage_name).unwrap_or("<invalid>"),
			array_bytes::bytes2hex("0x", &self.key)
		)
	}
}

impl core::fmt::Debug for TryDecodeEntireStorageError {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"key: {} value: {} info: {:?}",
			array_bytes::bytes2hex("0x", &self.key),
			array_bytes::bytes2hex("0x", self.raw.clone().unwrap_or_default()),
			self.info
		)
	}
}

/// Decode all the values based on the prefix of `info` to `V`.
///
/// Basically, it decodes and sums up all the values who's key start with `info.prefix`. For values,
/// this would be the value itself. For all sorts of maps, this should be all map items in the
/// absence of key collision.
fn decode_storage_info<V: Decode>(
	info: StorageInfo,
) -> Result<usize, Vec<TryDecodeEntireStorageError>> {
	let mut decoded = 0;

	let decode_key = |key: &[u8]| match sp_io::storage::get(key) {
		None => Ok(0),
		Some(bytes) => {
			let len = bytes.len();
			<V as DecodeAll>::decode_all(&mut bytes.as_ref()).map_err(|_| {
				TryDecodeEntireStorageError {
					key: key.to_vec(),
					raw: Some(bytes.to_vec()),
					info: info.clone(),
				}
			})?;

			Ok::<usize, _>(len)
		},
	};

	let mut errors = vec![];
	let mut next_key = Some(info.prefix.clone());
	loop {
		match next_key {
			Some(key) if key.starts_with(&info.prefix) => {
				match decode_key(&key) {
					Ok(bytes) => {
						decoded += bytes;
					},
					Err(e) => errors.push(e),
				};
				next_key = sp_io::storage::next_key(&key);
			},
			_ => break,
		}
	}

	if errors.is_empty() {
		Ok(decoded)
	} else {
		Err(errors)
	}
}

impl<Prefix, Value, QueryKind, OnEmpty> TryDecodeEntireStorage
	for crate::storage::types::StorageValue<Prefix, Value, QueryKind, OnEmpty>
where
	Prefix: StorageInstance,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let info = Self::partial_storage_info()
			.first()
			.cloned()
			.expect("Value has only one storage info");
		decode_storage_info::<Value>(info)
	}
}

impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues> TryDecodeEntireStorage
	for crate::storage::types::StorageMap<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Hasher: StorageHasher,
	Key: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let info = Self::partial_storage_info()
			.first()
			.cloned()
			.expect("Map has only one storage info");
		decode_storage_info::<Value>(info)
	}
}

impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues> TryDecodeEntireStorage
	for crate::storage::types::CountedStorageMap<
		Prefix,
		Hasher,
		Key,
		Value,
		QueryKind,
		OnEmpty,
		MaxValues,
	>
where
	Prefix: CountedStorageMapInstance,
	Hasher: StorageHasher,
	Key: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let (map_info, counter_info) = match &Self::partial_storage_info()[..] {
			[a, b] => (a.clone(), b.clone()),
			_ => panic!("Counted map has two storage info items"),
		};
		let mut decoded = decode_storage_info::<Counter>(counter_info)?;
		decoded += decode_storage_info::<Value>(map_info)?;
		Ok(decoded)
	}
}

impl<Prefix, Hasher1, Key1, Hasher2, Key2, Value, QueryKind, OnEmpty, MaxValues>
	TryDecodeEntireStorage
	for crate::storage::types::StorageDoubleMap<
		Prefix,
		Hasher1,
		Key1,
		Hasher2,
		Key2,
		Value,
		QueryKind,
		OnEmpty,
		MaxValues,
	>
where
	Prefix: StorageInstance,
	Hasher1: StorageHasher,
	Key1: FullCodec,
	Hasher2: StorageHasher,
	Key2: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let info = Self::partial_storage_info()
			.first()
			.cloned()
			.expect("Double-map has only one storage info");
		decode_storage_info::<Value>(info)
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues> TryDecodeEntireStorage
	for crate::storage::types::StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let info = Self::partial_storage_info()
			.first()
			.cloned()
			.expect("N-map has only one storage info");
		decode_storage_info::<Value>(info)
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues> TryDecodeEntireStorage
	for crate::storage::types::CountedStorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: CountedStorageNMapInstance,
	Key: KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn try_decode_entire_state() -> Result<usize, Vec<TryDecodeEntireStorageError>> {
		let (map_info, counter_info) = match &Self::partial_storage_info()[..] {
			[a, b] => (a.clone(), b.clone()),
			_ => panic!("Counted NMap has two storage info items"),
		};

		let mut decoded = decode_storage_info::<Counter>(counter_info)?;
		decoded += decode_storage_info::<Value>(map_info)?;
		Ok(decoded)
	}
}

#[cfg(test)]
mod tests {
}
