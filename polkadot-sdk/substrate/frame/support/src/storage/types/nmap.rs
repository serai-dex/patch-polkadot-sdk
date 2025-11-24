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

//! Storage n-map type. Particularly implements `StorageNMap` and `StoragePrefixedMap`
//! traits and their methods directly.

use crate::{
	storage::{
		types::{
			EncodeLikeTuple, HasKeyPrefix, HasReversibleKeyPrefix, OptionQuery, QueryKindTrait,
			TupleToEncodedIter,
		},
		KeyGenerator, PrefixIterator, StorageAppend, StorageDecodeLength, StoragePrefixedMap,
		StorageTryAppend,
	},
	traits::{Get, GetDefault, StorageInfo, StorageInstance},
};
use alloc::{vec, vec::Vec};
use codec::{Decode, Encode, EncodeLike, FullCodec, MaxEncodedLen};
use sp_runtime::SaturatedConversion;

/// A type representing an *NMap* in storage. This structure associates an arbitrary number of keys
/// with a value of a specified type stored on-chain.
///
/// For example, [`StorageDoubleMap`](frame_support::storage::types::StorageDoubleMap) is a special
/// case of an *NMap* with N = 2.
///
/// For general information regarding the `#[pallet::storage]` attribute, refer to
/// [`crate::pallet_macros::storage`].
///
/// # Example
///
/// ```
/// #[frame_support::pallet]
/// mod pallet {
///     # use frame_support::pallet_prelude::*;
///     # #[pallet::config]
///     # pub trait Config: frame_system::Config {}
///     # #[pallet::pallet]
///     # pub struct Pallet<T>(_);
/// 	/// A kitchen-sink StorageNMap, with all possible additional attributes.
///     #[pallet::storage]
/// 	#[pallet::getter(fn foo)]
/// 	#[pallet::storage_prefix = "OtherFoo"]
/// 	#[pallet::unbounded]
///     pub type Foo<T> = StorageNMap<
/// 		_,
/// 		(
/// 			NMapKey<Blake2_128Concat, u8>,
/// 			NMapKey<Identity, u16>,
/// 			NMapKey<Twox64Concat, u32>
/// 		),
/// 		u64,
/// 		ValueQuery,
/// 	>;
///
/// 	/// Named alternative syntax.
///     #[pallet::storage]
///     pub type Bar<T> = StorageNMap<
/// 		Key = (
/// 			NMapKey<Blake2_128Concat, u8>,
/// 			NMapKey<Identity, u16>,
/// 			NMapKey<Twox64Concat, u32>
/// 		),
/// 		Value = u64,
/// 		QueryKind = ValueQuery,
/// 	>;
/// }
/// ```
pub struct StorageNMap<
	Prefix,
	Key,
	Value,
	QueryKind = OptionQuery,
	OnEmpty = GetDefault,
	MaxValues = GetDefault,
>(core::marker::PhantomData<(Prefix, Key, Value, QueryKind, OnEmpty, MaxValues)>);

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
	crate::storage::generator::StorageNMap<Key, Value>
	for StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	type Query = QueryKind::Query;
	fn pallet_prefix() -> &'static [u8] {
		Prefix::pallet_prefix().as_bytes()
	}
	fn storage_prefix() -> &'static [u8] {
		Prefix::STORAGE_PREFIX.as_bytes()
	}
	fn prefix_hash() -> [u8; 32] {
		Prefix::prefix_hash()
	}
	fn from_optional_value_to_query(v: Option<Value>) -> Self::Query {
		QueryKind::from_optional_value_to_query(v)
	}
	fn from_query_to_optional_value(v: Self::Query) -> Option<Value> {
		QueryKind::from_query_to_optional_value(v)
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues> crate::storage::StoragePrefixedMap<Value>
	for StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn pallet_prefix() -> &'static [u8] {
		<Self as crate::storage::generator::StorageNMap<Key, Value>>::pallet_prefix()
	}
	fn storage_prefix() -> &'static [u8] {
		<Self as crate::storage::generator::StorageNMap<Key, Value>>::storage_prefix()
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
	StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	/// Get the storage key used to fetch a value corresponding to a specific key.
	pub fn hashed_key_for<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
	) -> Vec<u8> {
		<Self as crate::storage::StorageNMap<Key, Value>>::hashed_key_for(key)
	}

	/// Does the value (explicitly) exist in storage?
	pub fn contains_key<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(key: KArg) -> bool {
		<Self as crate::storage::StorageNMap<Key, Value>>::contains_key(key)
	}

	/// Load the value associated with the given key from the map.
	pub fn get<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
	) -> QueryKind::Query {
		<Self as crate::storage::StorageNMap<Key, Value>>::get(key)
	}

	/// Try to get the value for the given key from the map.
	///
	/// Returns `Ok` if it exists, `Err` if not.
	pub fn try_get<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
	) -> Result<Value, ()> {
		<Self as crate::storage::StorageNMap<Key, Value>>::try_get(key)
	}

	/// Store or remove the value to be associated with `key` so that `get` returns the `query`.
	pub fn set<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
		query: QueryKind::Query,
	) {
		<Self as crate::storage::StorageNMap<Key, Value>>::set(key, query)
	}

	/// Take a value from storage, removing it afterwards.
	pub fn take<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
	) -> QueryKind::Query {
		<Self as crate::storage::StorageNMap<Key, Value>>::take(key)
	}

	/// Swap the values of two key-pairs.
	pub fn swap<KOther, KArg1, KArg2>(key1: KArg1, key2: KArg2)
	where
		KOther: KeyGenerator,
		KArg1: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		KArg2: EncodeLikeTuple<KOther::KArg> + TupleToEncodedIter,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::swap::<KOther, _, _>(key1, key2)
	}

	/// Store a value to be associated with the given keys from the map.
	pub fn insert<KArg, VArg>(key: KArg, val: VArg)
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		VArg: EncodeLike<Value>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::insert(key, val)
	}

	/// Remove the value under the given keys.
	pub fn remove<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(key: KArg) {
		<Self as crate::storage::StorageNMap<Key, Value>>::remove(key)
	}

	/// Remove all values starting with `partial_key` in the overlay and up to `limit` in the
	/// backend.
	///
	/// All values in the client overlay will be deleted, if there is some `limit` then up to
	/// `limit` values are deleted from the client backend, if `limit` is none then all values in
	/// the client backend are deleted.
	///
	/// # Note
	///
	/// Calling this multiple times per block with a `limit` set leads always to the same keys being
	/// removed and the same result being returned. This happens because the keys to delete in the
	/// overlay are not taken into account when deleting keys in the backend.
	#[deprecated = "Use `clear_prefix` instead"]
	pub fn remove_prefix<KP>(partial_key: KP, limit: Option<u32>) -> sp_io::KillStorageResult
	where
		Key: HasKeyPrefix<KP>,
	{
		#[allow(deprecated)]
		<Self as crate::storage::StorageNMap<Key, Value>>::remove_prefix(partial_key, limit)
	}

	/// Attempt to remove items from the map matching a `partial_key` prefix.
	///
	/// Returns [`MultiRemovalResults`](sp_io::MultiRemovalResults) to inform about the result. Once
	/// the resultant `maybe_cursor` field is `None`, then no further items remain to be deleted.
	///
	/// NOTE: After the initial call for any given map, it is important that no further items
	/// are inserted into the map which match the `partial key`. If so, then the map may not be
	/// empty when the resultant `maybe_cursor` is `None`.
	///
	/// # Limit
	///
	/// A `limit` must be provided in order to cap the maximum
	/// amount of deletions done in a single call. This is one fewer than the
	/// maximum number of backend iterations which may be done by this operation and as such
	/// represents the maximum number of backend deletions which may happen. A `limit` of zero
	/// implies that no keys will be deleted, though there may be a single iteration done.
	///
	/// # Cursor
	///
	/// A *cursor* may be passed in to this operation with `maybe_cursor`. `None` should only be
	/// passed once (in the initial call) for any given storage map and `partial_key`. Subsequent
	/// calls operating on the same map/`partial_key` should always pass `Some`, and this should be
	/// equal to the previous call result's `maybe_cursor` field.
	pub fn clear_prefix<KP>(
		partial_key: KP,
		limit: u32,
		maybe_cursor: Option<&[u8]>,
	) -> sp_io::MultiRemovalResults
	where
		Key: HasKeyPrefix<KP>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::clear_prefix(
			partial_key,
			limit,
			maybe_cursor,
		)
	}

	/// Iterate over values that share the first key.
	pub fn iter_prefix_values<KP>(partial_key: KP) -> PrefixIterator<Value>
	where
		Key: HasKeyPrefix<KP>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::iter_prefix_values(partial_key)
	}

	/// Mutate the value under the given keys.
	pub fn mutate<KArg, R, F>(key: KArg, f: F) -> R
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		F: FnOnce(&mut QueryKind::Query) -> R,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::mutate(key, f)
	}

	/// Mutate the value under the given keys when the closure returns `Ok`.
	pub fn try_mutate<KArg, R, E, F>(key: KArg, f: F) -> Result<R, E>
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		F: FnOnce(&mut QueryKind::Query) -> Result<R, E>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::try_mutate(key, f)
	}

	/// Mutate the value under the given keys. Deletes the item if mutated to a `None`.
	pub fn mutate_exists<KArg, R, F>(key: KArg, f: F) -> R
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		F: FnOnce(&mut Option<Value>) -> R,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::mutate_exists(key, f)
	}

	/// Mutate the item, only if an `Ok` value is returned. Deletes the item if mutated to a `None`.
	/// `f` will always be called with an option representing if the storage item exists (`Some<V>`)
	/// or if the storage item does not exist (`None`), independent of the `QueryType`.
	pub fn try_mutate_exists<KArg, R, E, F>(key: KArg, f: F) -> Result<R, E>
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		F: FnOnce(&mut Option<Value>) -> Result<R, E>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::try_mutate_exists(key, f)
	}

	/// Append the given item to the value in the storage.
	///
	/// `Value` is required to implement [`StorageAppend`].
	///
	/// # Warning
	///
	/// If the storage item is not encoded properly, the storage will be overwritten
	/// and set to `[item]`. Any default value set for the storage item will be ignored
	/// on overwrite.
	pub fn append<Item, EncodeLikeItem, KArg>(key: KArg, item: EncodeLikeItem)
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
		Item: Encode,
		EncodeLikeItem: EncodeLike<Item>,
		Value: StorageAppend<Item>,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::append(key, item)
	}

	/// Try and append the given item to the value in the storage.
	///
	/// Is only available if `Value` of the storage implements [`StorageTryAppend`].
	pub fn try_append<KArg, Item, EncodeLikeItem>(key: KArg, item: EncodeLikeItem) -> Result<(), ()>
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter + Clone,
		Item: Encode,
		EncodeLikeItem: EncodeLike<Item>,
		Value: StorageTryAppend<Item>,
	{
		<Self as crate::storage::TryAppendNMap<Key, Value, Item>>::try_append(key, item)
	}

	/// Read the length of the storage value without decoding the entire value under the
	/// given `key1` and `key2`.
	///
	/// `Value` is required to implement [`StorageDecodeLength`].
	///
	/// If the value does not exists or it fails to decode the length, `None` is returned.
	/// Otherwise `Some(len)` is returned.
	///
	/// # Warning
	///
	/// `None` does not mean that `get()` does not return a value. The default value is completely
	/// ignored by this function.
	pub fn decode_len<KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter>(
		key: KArg,
	) -> Option<usize>
	where
		Value: StorageDecodeLength,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::decode_len(key)
	}

	/// Migrate an item with the given `key` from defunct `hash_fns` to the current hashers.
	///
	/// If the key doesn't exist, then it's a no-op. If it does, then it returns its value.
	pub fn migrate_keys<KArg>(key: KArg, hash_fns: Key::HArg) -> Option<Value>
	where
		KArg: EncodeLikeTuple<Key::KArg> + TupleToEncodedIter,
	{
		<Self as crate::storage::StorageNMap<Key, Value>>::migrate_keys::<_>(key, hash_fns)
	}

	/// Remove all values in the overlay and up to `limit` in the backend.
	///
	/// All values in the client overlay will be deleted, if there is some `limit` then up to
	/// `limit` values are deleted from the client backend, if `limit` is none then all values in
	/// the client backend are deleted.
	///
	/// # Note
	///
	/// Calling this multiple times per block with a `limit` set leads always to the same keys being
	/// removed and the same result being returned. This happens because the keys to delete in the
	/// overlay are not taken into account when deleting keys in the backend.
	#[deprecated = "Use `clear` instead"]
	pub fn remove_all(limit: Option<u32>) -> sp_io::KillStorageResult {
		#[allow(deprecated)]
		<Self as crate::storage::StoragePrefixedMap<Value>>::remove_all(limit).into()
	}

	/// Attempt to remove all items from the map.
	///
	/// Returns [`MultiRemovalResults`](sp_io::MultiRemovalResults) to inform about the result. Once
	/// the resultant `maybe_cursor` field is `None`, then no further items remain to be deleted.
	///
	/// NOTE: After the initial call for any given map, it is important that no further items
	/// are inserted into the map. If so, then the map may not be empty when the resultant
	/// `maybe_cursor` is `None`.
	///
	/// # Limit
	///
	/// A `limit` must always be provided through in order to cap the maximum
	/// amount of deletions done in a single call. This is one fewer than the
	/// maximum number of backend iterations which may be done by this operation and as such
	/// represents the maximum number of backend deletions which may happen. A `limit` of zero
	/// implies that no keys will be deleted, though there may be a single iteration done.
	///
	/// # Cursor
	///
	/// A *cursor* may be passed in to this operation with `maybe_cursor`. `None` should only be
	/// passed once (in the initial call) for any given storage map. Subsequent calls
	/// operating on the same map should always pass `Some`, and this should be equal to the
	/// previous call result's `maybe_cursor` field.
	pub fn clear(limit: u32, maybe_cursor: Option<&[u8]>) -> sp_io::MultiRemovalResults {
		<Self as crate::storage::StoragePrefixedMap<Value>>::clear(limit, maybe_cursor)
	}

	/// Iter over all value of the storage.
	///
	/// NOTE: If a value failed to decode because storage is corrupted then it is skipped.
	pub fn iter_values() -> crate::storage::PrefixIterator<Value> {
		<Self as crate::storage::StoragePrefixedMap<Value>>::iter_values()
	}

	/// Translate the values of all elements by a function `f`, in the map in no particular order.
	/// By returning `None` from `f` for an element, you'll remove it from the map.
	///
	/// NOTE: If a value fail to decode because storage is corrupted then it is skipped.
	///
	/// # Warning
	///
	/// This function must be used with care, before being updated the storage still contains the
	/// old type, thus other calls (such as `get`) will fail at decoding it.
	///
	/// # Usage
	///
	/// This would typically be called inside the module implementation of on_runtime_upgrade.
	pub fn translate_values<OldValue: Decode, F: FnMut(OldValue) -> Option<Value>>(f: F) {
		<Self as crate::storage::StoragePrefixedMap<Value>>::translate_values(f)
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
	StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::ReversibleKeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	/// Enumerate all elements in the map with prefix key `kp` in no particular order.
	///
	/// If you add or remove values whose prefix key is `kp` to the map while doing this, you'll get
	/// undefined results.
	pub fn iter_prefix<KP>(
		kp: KP,
	) -> crate::storage::PrefixIterator<(<Key as HasKeyPrefix<KP>>::Suffix, Value)>
	where
		Key: HasReversibleKeyPrefix<KP>,
	{
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_prefix(kp)
	}

	/// Enumerate all elements in the map with prefix key `kp` after a specified `starting_raw_key`
	/// in no particular order.
	///
	/// If you add or remove values whose prefix key is `kp` to the map while doing this, you'll get
	/// undefined results.
	pub fn iter_prefix_from<KP>(
		kp: KP,
		starting_raw_key: Vec<u8>,
	) -> crate::storage::PrefixIterator<(<Key as HasKeyPrefix<KP>>::Suffix, Value)>
	where
		Key: HasReversibleKeyPrefix<KP>,
	{
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_prefix_from(
			kp,
			starting_raw_key,
		)
	}

	/// Enumerate all suffix keys in the map with prefix key `kp` in no particular order.
	///
	/// If you add or remove values whose prefix key is `kp` to the map while doing this, you'll get
	/// undefined results.
	pub fn iter_key_prefix<KP>(
		kp: KP,
	) -> crate::storage::KeyPrefixIterator<<Key as HasKeyPrefix<KP>>::Suffix>
	where
		Key: HasReversibleKeyPrefix<KP>,
	{
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_key_prefix(kp)
	}

	/// Enumerate all suffix keys in the map with prefix key `kp` after a specified
	/// `starting_raw_key` in no particular order.
	///
	/// If you add or remove values whose prefix key is `kp` to the map while doing this, you'll get
	/// undefined results.
	pub fn iter_key_prefix_from<KP>(
		kp: KP,
		starting_raw_key: Vec<u8>,
	) -> crate::storage::KeyPrefixIterator<<Key as HasKeyPrefix<KP>>::Suffix>
	where
		Key: HasReversibleKeyPrefix<KP>,
	{
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_key_prefix_from(
			kp,
			starting_raw_key,
		)
	}

	/// Remove all elements from the map with prefix key `kp` and iterate through them in no
	/// particular order.
	///
	/// If you add elements with prefix key `k1` to the map while doing this, you'll get undefined
	/// results.
	pub fn drain_prefix<KP>(
		kp: KP,
	) -> crate::storage::PrefixIterator<(<Key as HasKeyPrefix<KP>>::Suffix, Value)>
	where
		Key: HasReversibleKeyPrefix<KP>,
	{
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::drain_prefix(kp)
	}

	/// Enumerate all elements in the map in no particular order.
	///
	/// If you add or remove values to the map while doing this, you'll get undefined results.
	pub fn iter() -> crate::storage::PrefixIterator<(Key::Key, Value)> {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter()
	}

	/// Enumerate all elements in the map after a specified `starting_key` in no particular order.
	///
	/// If you add or remove values to the map while doing this, you'll get undefined results.
	pub fn iter_from(
		starting_raw_key: Vec<u8>,
	) -> crate::storage::PrefixIterator<(Key::Key, Value)> {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_from(starting_raw_key)
	}

	/// Enumerate all keys in the map in no particular order.
	///
	/// If you add or remove values to the map while doing this, you'll get undefined results.
	pub fn iter_keys() -> crate::storage::KeyPrefixIterator<Key::Key> {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_keys()
	}

	/// Enumerate all keys in the map after a specified `starting_raw_key` in no particular order.
	///
	/// If you add or remove values to the map while doing this, you'll get undefined results.
	pub fn iter_keys_from(
		starting_raw_key: Vec<u8>,
	) -> crate::storage::KeyPrefixIterator<Key::Key> {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::iter_keys_from(starting_raw_key)
	}

	/// Remove all elements from the map and iterate through them in no particular order.
	///
	/// If you add elements to the map while doing this, you'll get undefined results.
	pub fn drain() -> crate::storage::PrefixIterator<(Key::Key, Value)> {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::drain()
	}

	/// Translate the values of all elements by a function `f`, in the map in no particular order.
	///
	/// By returning `None` from `f` for an element, you'll remove it from the map.
	///
	/// NOTE: If a value fail to decode because storage is corrupted then it is skipped.
	pub fn translate<O: Decode, F: FnMut(Key::Key, O) -> Option<Value>>(f: F) {
		<Self as crate::storage::IterableStorageNMap<Key, Value>>::translate(f)
	}
}

impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues> crate::traits::StorageInfoTrait
	for StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::KeyGenerator + super::key::KeyGeneratorMaxEncodedLen,
	Value: FullCodec + MaxEncodedLen,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn storage_info() -> Vec<StorageInfo> {
		vec![StorageInfo {
			pallet_name: Self::pallet_prefix().to_vec(),
			storage_name: Self::storage_prefix().to_vec(),
			prefix: Self::final_prefix().to_vec(),
			max_values: MaxValues::get(),
			max_size: Some(
				Key::key_max_encoded_len()
					.saturating_add(Value::max_encoded_len())
					.saturated_into(),
			),
		}]
	}
}

/// It doesn't require to implement `MaxEncodedLen` and give no information for `max_size`.
impl<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues> crate::traits::PartialStorageInfoTrait
	for StorageNMap<Prefix, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: StorageInstance,
	Key: super::key::KeyGenerator,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn partial_storage_info() -> Vec<StorageInfo> {
		vec![StorageInfo {
			pallet_name: Self::pallet_prefix().to_vec(),
			storage_name: Self::storage_prefix().to_vec(),
			prefix: Self::final_prefix().to_vec(),
			max_values: MaxValues::get(),
			max_size: None,
		}]
	}
}
#[cfg(test)]
mod test {
}
