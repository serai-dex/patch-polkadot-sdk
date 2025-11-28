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

//! Storage counted map type.

use crate::{
	storage::{
		generator::StorageMap as _,
		types::{
			OptionQuery, QueryKindTrait, StorageMap, StorageValue,
			ValueQuery,
		},
		StorageAppend, StorageDecodeLength, StorageTryAppend,
	},
	traits::{Get, GetDefault, StorageInfo, StorageInfoTrait, StorageInstance},
	Never,
};
use alloc::{vec, vec::Vec};
use codec::{Decode, Encode, EncodeLike, FullCodec, MaxEncodedLen, Ref};
use sp_io::MultiRemovalResults;
use sp_runtime::traits::Saturating;

/// A wrapper around a [`StorageMap`] and a [`StorageValue`] (with the value being `u32`) to keep
/// track of how many items are in a map, without needing to iterate all the values.
///
/// This storage item has additional storage read and write overhead when manipulating values
/// compared to a regular storage map.
///
/// For functions where we only add or remove a value, a single storage read is needed to check if
/// that value already exists. For mutate functions, two storage reads are used to check if the
/// value existed before and after the mutation.
///
/// Whenever the counter needs to be updated, an additional read and write occurs to update that
/// counter.
///
/// The total number of items currently stored in the map can be retrieved with the
/// [`CountedStorageMap::count`] method.
///
/// For general information regarding the `#[pallet::storage]` attribute, refer to
/// [`crate::pallet_macros::storage`].
///
/// # Examples
///
/// Declaring a counted map:
///
/// ```
/// #[frame_support::pallet]
/// mod pallet {
/// # 	use frame_support::pallet_prelude::*;
/// # 	#[pallet::config]
/// # 	pub trait Config: frame_system::Config {}
/// # 	#[pallet::pallet]
/// # 	pub struct Pallet<T>(_);
/// 	/// A kitchen-sink CountedStorageMap, with all possible additional attributes.
///     #[pallet::storage]
/// 	#[pallet::getter(fn foo)]
/// 	#[pallet::storage_prefix = "OtherFoo"]
/// 	#[pallet::unbounded]
///     pub type Foo<T> = CountedStorageMap<
/// 		_,
/// 		Blake2_128Concat,
/// 		u32,
/// 		u32,
/// 		ValueQuery,
/// 	>;
///
/// 	/// Alternative named syntax.
/// 	#[pallet::storage]
///     pub type Bar<T> = CountedStorageMap<
/// 		Hasher = Blake2_128Concat,
/// 		Key = u32,
/// 		Value = u32,
/// 		QueryKind = ValueQuery
/// 	>;
/// }
/// ```
///
/// Using a counted map in action:
pub struct CountedStorageMap<
	Prefix,
	Hasher,
	Key,
	Value,
	QueryKind = OptionQuery,
	OnEmpty = GetDefault,
	MaxValues = GetDefault,
>(core::marker::PhantomData<(Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues)>);

/// The requirement for an instance of [`CountedStorageMap`].
pub trait CountedStorageMapInstance: StorageInstance {
	/// The prefix to use for the counter storage value.
	type CounterPrefix: StorageInstance;
}

// Private helper trait to access map from counted storage map.
trait MapWrapper {
	type Map;
}

impl<P: CountedStorageMapInstance, H, K, V, Q, O, M> MapWrapper
	for CountedStorageMap<P, H, K, V, Q, O, M>
{
	type Map = StorageMap<P, H, K, V, Q, O, M>;
}

/// The numeric counter type.
pub type Counter = u32;

type CounterFor<P> =
	StorageValue<<P as CountedStorageMapInstance>::CounterPrefix, Counter, ValueQuery>;

/// On removal logic for updating counter while draining upon some prefix with
/// [`crate::storage::PrefixIterator`].
pub struct OnRemovalCounterUpdate<Prefix>(core::marker::PhantomData<Prefix>);

impl<Prefix: CountedStorageMapInstance> crate::storage::PrefixIteratorOnRemoval
	for OnRemovalCounterUpdate<Prefix>
{
	fn on_removal(_key: &[u8], _value: &[u8]) {
		CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
	}
}

impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
	CountedStorageMap<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: CountedStorageMapInstance,
	Hasher: crate::hash::StorageHasher,
	Key: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	/// The key used to store the counter of the map.
	pub fn counter_storage_final_key() -> [u8; 32] {
		CounterFor::<Prefix>::hashed_key()
	}

	/// The prefix used to generate the key of the map.
	pub fn map_storage_final_prefix() -> Vec<u8> {
		use crate::storage::generator::StorageMap;
		<Self as MapWrapper>::Map::prefix_hash().to_vec()
	}

	/// Get the storage key used to fetch a value corresponding to a specific key.
	pub fn hashed_key_for<KeyArg: EncodeLike<Key>>(key: KeyArg) -> Vec<u8> {
		<Self as MapWrapper>::Map::hashed_key_for(key)
	}

	/// Does the value (explicitly) exist in storage?
	pub fn contains_key<KeyArg: EncodeLike<Key>>(key: KeyArg) -> bool {
		<Self as MapWrapper>::Map::contains_key(key)
	}

	/// Load the value associated with the given key from the map.
	pub fn get<KeyArg: EncodeLike<Key>>(key: KeyArg) -> QueryKind::Query {
		<Self as MapWrapper>::Map::get(key)
	}

	/// Try to get the value for the given key from the map.
	///
	/// Returns `Ok` if it exists, `Err` if not.
	pub fn try_get<KeyArg: EncodeLike<Key>>(key: KeyArg) -> Result<Value, ()> {
		<Self as MapWrapper>::Map::try_get(key)
	}

	/// Store or remove the value to be associated with `key` so that `get` returns the `query`.
	pub fn set<KeyArg: EncodeLike<Key>>(key: KeyArg, q: QueryKind::Query) {
		match QueryKind::from_query_to_optional_value(q) {
			Some(v) => Self::insert(key, v),
			None => Self::remove(key),
		}
	}

	/// Swap the values of two keys.
	pub fn swap<KeyArg1: EncodeLike<Key>, KeyArg2: EncodeLike<Key>>(key1: KeyArg1, key2: KeyArg2) {
		<Self as MapWrapper>::Map::swap(key1, key2)
	}

	/// Store a value to be associated with the given key from the map.
	pub fn insert<KeyArg: EncodeLike<Key>, ValArg: EncodeLike<Value>>(key: KeyArg, val: ValArg) {
		if !<Self as MapWrapper>::Map::contains_key(Ref::from(&key)) {
			CounterFor::<Prefix>::mutate(|value| value.saturating_inc());
		}
		<Self as MapWrapper>::Map::insert(key, val)
	}

	/// Remove the value under a key.
	pub fn remove<KeyArg: EncodeLike<Key>>(key: KeyArg) {
		if <Self as MapWrapper>::Map::contains_key(Ref::from(&key)) {
			CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
		}
		<Self as MapWrapper>::Map::remove(key)
	}

	/// Mutate the value under a key.
	pub fn mutate<KeyArg: EncodeLike<Key>, R, F: FnOnce(&mut QueryKind::Query) -> R>(
		key: KeyArg,
		f: F,
	) -> R {
		Self::try_mutate(key, |v| Ok::<R, Never>(f(v)))
			.expect("`Never` can not be constructed")
	}

	/// Mutate the item, only if an `Ok` value is returned.
	pub fn try_mutate<KeyArg, R, E, F>(key: KeyArg, f: F) -> Result<R, E>
	where
		KeyArg: EncodeLike<Key>,
		F: FnOnce(&mut QueryKind::Query) -> Result<R, E>,
	{
		Self::try_mutate_exists(key, |option_value_ref| {
			let option_value = core::mem::replace(option_value_ref, None);
			let mut query = <Self as MapWrapper>::Map::from_optional_value_to_query(option_value);
			let res = f(&mut query);
			let option_value = <Self as MapWrapper>::Map::from_query_to_optional_value(query);
			let _ = core::mem::replace(option_value_ref, option_value);
			res
		})
	}

	/// Mutate the value under a key. Deletes the item if mutated to a `None`.
	pub fn mutate_exists<KeyArg: EncodeLike<Key>, R, F: FnOnce(&mut Option<Value>) -> R>(
		key: KeyArg,
		f: F,
	) -> R {
		Self::try_mutate_exists(key, |v| Ok::<R, Never>(f(v)))
			.expect("`Never` can not be constructed")
	}

	/// Mutate the item, only if an `Ok` value is returned. Deletes the item if mutated to a `None`.
	/// `f` will always be called with an option representing if the storage item exists (`Some<V>`)
	/// or if the storage item does not exist (`None`), independent of the `QueryType`.
	pub fn try_mutate_exists<KeyArg, R, E, F>(key: KeyArg, f: F) -> Result<R, E>
	where
		KeyArg: EncodeLike<Key>,
		F: FnOnce(&mut Option<Value>) -> Result<R, E>,
	{
		<Self as MapWrapper>::Map::try_mutate_exists(key, |option_value| {
			let existed = option_value.is_some();
			let res = f(option_value);
			let exist = option_value.is_some();

			if res.is_ok() {
				if existed && !exist {
					// Value was deleted
					CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
				} else if !existed && exist {
					// Value was added
					CounterFor::<Prefix>::mutate(|value| value.saturating_inc());
				}
			}
			res
		})
	}

	/// Take the value under a key.
	pub fn take<KeyArg: EncodeLike<Key>>(key: KeyArg) -> QueryKind::Query {
		let removed_value = <Self as MapWrapper>::Map::mutate_exists(key, |value| value.take());
		if removed_value.is_some() {
			CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
		}
		<Self as MapWrapper>::Map::from_optional_value_to_query(removed_value)
	}

	/// Append the given items to the value in the storage.
	///
	/// `Value` is required to implement `codec::EncodeAppend`.
	///
	/// # Warning
	///
	/// If the storage item is not encoded properly, the storage will be overwritten and set to
	/// `[item]`. Any default value set for the storage item will be ignored on overwrite.
	pub fn append<Item, EncodeLikeItem, EncodeLikeKey>(key: EncodeLikeKey, item: EncodeLikeItem)
	where
		EncodeLikeKey: EncodeLike<Key>,
		Item: Encode,
		EncodeLikeItem: EncodeLike<Item>,
		Value: StorageAppend<Item>,
	{
		if !<Self as MapWrapper>::Map::contains_key(Ref::from(&key)) {
			CounterFor::<Prefix>::mutate(|value| value.saturating_inc());
		}
		<Self as MapWrapper>::Map::append(key, item)
	}

	/// Read the length of the storage value without decoding the entire value under the given
	/// `key`.
	///
	/// `Value` is required to implement [`StorageDecodeLength`].
	///
	/// If the value does not exists or it fails to decode the length, `None` is returned. Otherwise
	/// `Some(len)` is returned.
	///
	/// # Warning
	///
	/// `None` does not mean that `get()` does not return a value. The default value is completely
	/// ignored by this function.
	pub fn decode_len<KeyArg: EncodeLike<Key>>(key: KeyArg) -> Option<usize>
	where
		Value: StorageDecodeLength,
	{
		<Self as MapWrapper>::Map::decode_len(key)
	}

	/// Migrate an item with the given `key` from a defunct `OldHasher` to the current hasher.
	///
	/// If the key doesn't exist, then it's a no-op. If it does, then it returns its value.
	pub fn migrate_key<OldHasher: crate::hash::StorageHasher, KeyArg: EncodeLike<Key>>(
		key: KeyArg,
	) -> Option<Value> {
		<Self as MapWrapper>::Map::migrate_key::<OldHasher, _>(key)
	}

	/// Remove all values in the map.
	#[deprecated = "Use `clear` instead"]
	pub fn remove_all() {
		#[allow(deprecated)]
		<Self as MapWrapper>::Map::remove_all(None);
		CounterFor::<Prefix>::kill();
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
	pub fn clear(limit: u32, maybe_cursor: Option<&[u8]>) -> MultiRemovalResults {
		let result = <Self as MapWrapper>::Map::clear(limit, maybe_cursor);
		match result.maybe_cursor {
			None => CounterFor::<Prefix>::kill(),
			Some(_) => CounterFor::<Prefix>::mutate(|x| x.saturating_reduce(result.unique)),
		}
		result
	}

	/// Iter over all value of the storage.
	///
	/// NOTE: If a value failed to decode because storage is corrupted then it is skipped.
	pub fn iter_values() -> crate::storage::PrefixIterator<Value, OnRemovalCounterUpdate<Prefix>> {
		<Self as MapWrapper>::Map::iter_values().convert_on_removal()
	}

	/// Translate the values of all elements by a function `f`, in the map in no particular order.
	///
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
	pub fn translate_values<OldValue: Decode, F: FnMut(OldValue) -> Option<Value>>(mut f: F) {
		<Self as MapWrapper>::Map::translate_values(|old_value| {
			let res = f(old_value);
			if res.is_none() {
				CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
			}
			res
		})
	}

	/// Try and append the given item to the value in the storage.
	///
	/// Is only available if `Value` of the storage implements [`StorageTryAppend`].
	pub fn try_append<KArg, Item, EncodeLikeItem>(key: KArg, item: EncodeLikeItem) -> Result<(), ()>
	where
		KArg: EncodeLike<Key>,
		Item: Encode,
		EncodeLikeItem: EncodeLike<Item>,
		Value: StorageTryAppend<Item>,
	{
		let bound = Value::bound();
		let current = <Self as MapWrapper>::Map::decode_len(Ref::from(&key)).unwrap_or_default();
		if current < bound {
			CounterFor::<Prefix>::mutate(|value| value.saturating_inc());
			let key = <Self as MapWrapper>::Map::hashed_key_for(key);
			sp_io::storage::append(&key, item.encode());
			Ok(())
		} else {
			Err(())
		}
	}

	/// Initialize the counter with the actual number of items in the map.
	///
	/// This function iterates through all the items in the map and sets the counter. This operation
	/// can be very heavy, so use with caution.
	///
	/// Returns the number of items in the map which is used to set the counter.
	pub fn initialize_counter() -> Counter {
		let count = Self::iter_values().count() as Counter;
		CounterFor::<Prefix>::set(count);
		count
	}

	/// Return the count.
	pub fn count() -> Counter {
		CounterFor::<Prefix>::get()
	}
}

impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
	CountedStorageMap<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: CountedStorageMapInstance,
	Hasher: crate::hash::StorageHasher + crate::ReversibleStorageHasher,
	Key: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	/// Enumerate all elements in the map in no particular order.
	///
	/// If you alter the map while doing this, you'll get undefined results.
	pub fn iter() -> crate::storage::PrefixIterator<(Key, Value), OnRemovalCounterUpdate<Prefix>> {
		<Self as MapWrapper>::Map::iter().convert_on_removal()
	}

	/// Remove all elements from the map and iterate through them in no particular order.
	///
	/// If you add elements to the map while doing this, you'll get undefined results.
	pub fn drain() -> crate::storage::PrefixIterator<(Key, Value), OnRemovalCounterUpdate<Prefix>> {
		<Self as MapWrapper>::Map::drain().convert_on_removal()
	}

	/// Translate the values of all elements by a function `f`, in the map in no particular order.
	///
	/// By returning `None` from `f` for an element, you'll remove it from the map.
	///
	/// NOTE: If a value fail to decode because storage is corrupted then it is skipped.
	pub fn translate<O: Decode, F: FnMut(Key, O) -> Option<Value>>(mut f: F) {
		<Self as MapWrapper>::Map::translate(|key, old_value| {
			let res = f(key, old_value);
			if res.is_none() {
				CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
			}
			res
		})
	}

	/// Enumerate all elements in the counted map after a specified `starting_raw_key` in no
	/// particular order.
	///
	/// If you alter the map while doing this, you'll get undefined results.
	pub fn iter_from(
		starting_raw_key: Vec<u8>,
	) -> crate::storage::PrefixIterator<(Key, Value), OnRemovalCounterUpdate<Prefix>> {
		<Self as MapWrapper>::Map::iter_from(starting_raw_key).convert_on_removal()
	}

	/// Enumerate all keys in the counted map.
	///
	/// If you alter the map while doing this, you'll get undefined results.
	pub fn iter_keys() -> crate::storage::KeyPrefixIterator<Key> {
		<Self as MapWrapper>::Map::iter_keys()
	}
}

impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues> crate::traits::StorageInfoTrait
	for CountedStorageMap<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: CountedStorageMapInstance,
	Hasher: crate::hash::StorageHasher,
	Key: FullCodec + MaxEncodedLen,
	Value: FullCodec + MaxEncodedLen,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn storage_info() -> Vec<StorageInfo> {
		[<Self as MapWrapper>::Map::storage_info(), CounterFor::<Prefix>::storage_info()].concat()
	}
}

/// It doesn't require to implement `MaxEncodedLen` and give no information for `max_size`.
impl<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
	crate::traits::PartialStorageInfoTrait
	for CountedStorageMap<Prefix, Hasher, Key, Value, QueryKind, OnEmpty, MaxValues>
where
	Prefix: CountedStorageMapInstance,
	Hasher: crate::hash::StorageHasher,
	Key: FullCodec,
	Value: FullCodec,
	QueryKind: QueryKindTrait<Value, OnEmpty>,
	OnEmpty: Get<QueryKind::Query> + 'static,
	MaxValues: Get<Option<u32>>,
{
	fn partial_storage_info() -> Vec<StorageInfo> {
		[<Self as MapWrapper>::Map::partial_storage_info(), CounterFor::<Prefix>::storage_info()]
			.concat()
	}
}

#[cfg(test)]
mod test {
}
