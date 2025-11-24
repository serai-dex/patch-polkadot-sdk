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

use crate::{BoundedBTreeMap, BoundedBTreeSet, BoundedVec, WeakBoundedVec};
use alloc::vec::Vec;
use codec::Decode;

/// Provides the sealed trait `StreamIter`.
mod private {
	use super::*;

	/// Used as marker trait for types that support stream iteration.
	pub trait StreamIter {
		/// The actual iterator implementation.
		type Iterator: core::iter::Iterator;

		/// Create the stream iterator for the value found at `key`.
		fn stream_iter(key: Vec<u8>) -> Self::Iterator;
	}

	impl<T: codec::Decode> StreamIter for Vec<T> {
		type Iterator = ScaleContainerStreamIter<T>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<T: codec::Decode> StreamIter for alloc::collections::btree_set::BTreeSet<T> {
		type Iterator = ScaleContainerStreamIter<T>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<K: codec::Decode, V: codec::Decode> StreamIter
		for alloc::collections::btree_map::BTreeMap<K, V>
	{
		type Iterator = ScaleContainerStreamIter<(K, V)>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<T: codec::Decode, S> StreamIter for BoundedVec<T, S> {
		type Iterator = ScaleContainerStreamIter<T>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<T: codec::Decode, S> StreamIter for WeakBoundedVec<T, S> {
		type Iterator = ScaleContainerStreamIter<T>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<K: codec::Decode, V: codec::Decode, S> StreamIter for BoundedBTreeMap<K, V, S> {
		type Iterator = ScaleContainerStreamIter<(K, V)>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}

	impl<T: codec::Decode, S> StreamIter for BoundedBTreeSet<T, S> {
		type Iterator = ScaleContainerStreamIter<T>;

		fn stream_iter(key: Vec<u8>) -> Self::Iterator {
			ScaleContainerStreamIter::new(key)
		}
	}
}

/// An iterator that streams values directly from storage.
///
/// Requires that `T` implements the sealed trait `StreamIter`.
///
/// Instead of loading the entire `T` into memory, the iterator only loads a certain number of bytes
/// into memory to decode the next `T::Item`. The iterator implementation is allowed to have some
/// internal buffer to reduce the number of storage reads. The iterator should have an almost
/// constant memory usage over its lifetime. If at some point there is a decoding error, the
/// iterator will return `None` to signal that the iterator is finished.
pub trait StorageStreamIter<T: private::StreamIter> {
	/// Create the streaming iterator.
	fn stream_iter() -> T::Iterator;
}

impl<T: private::StreamIter + codec::FullCodec, StorageValue: super::StorageValue<T>>
	StorageStreamIter<T> for StorageValue
{
	fn stream_iter() -> T::Iterator {
		T::stream_iter(Self::hashed_key().into())
	}
}

/// A streaming iterator implementation for SCALE container types.
///
/// SCALE container types follow the same type of encoding `Compact<u32>(len) ++ data`.
/// This type provides an [`Iterator`](core::iter::Iterator) implementation that decodes
/// one item after another with each call to [`next`](Self::next). The bytes representing
/// the container are also not read at once into memory and instead being read in chunks. As long
/// as individual items are smaller than these chunks the memory usage of this iterator should
/// be constant. On decoding errors [`next`](Self::next) will return `None` to signal that the
/// iterator is finished.
pub struct ScaleContainerStreamIter<T> {
	marker: core::marker::PhantomData<T>,
	input: StorageInput,
	length: u32,
	read: u32,
}

impl<T> ScaleContainerStreamIter<T> {
	/// Creates a new instance of the stream iterator.
	///
	/// - `key`: Storage key of the container in the state.
	///
	/// Same as [`Self::new_try`], but logs a potential error and sets the length to `0`.
	pub fn new(key: Vec<u8>) -> Self {
		let mut input = StorageInput::new(key);
		let length = if input.exists() {
			match codec::Compact::<u32>::decode(&mut input) {
				Ok(length) => length.0,
				Err(e) => {
					// TODO #3700: error should be handleable.
					log::error!(
						target: "runtime::storage",
						"Corrupted state at `{:?}`: failed to decode element count: {:?}",
						input.key,
						e,
					);

					0
				},
			}
		} else {
			0
		};

		Self { marker: core::marker::PhantomData, input, length, read: 0 }
	}

	/// Creates a new instance of the stream iterator.
	///
	/// - `key`: Storage key of the container in the state.
	///
	/// Returns an error if the length of the container fails to decode.
	pub fn new_try(key: Vec<u8>) -> Result<Self, codec::Error> {
		let mut input = StorageInput::new(key);
		let length = if input.exists() { codec::Compact::<u32>::decode(&mut input)?.0 } else { 0 };

		Ok(Self { marker: core::marker::PhantomData, input, length, read: 0 })
	}
}

impl<T: codec::Decode> core::iter::Iterator for ScaleContainerStreamIter<T> {
	type Item = T;

	fn next(&mut self) -> Option<T> {
		if self.read >= self.length {
			return None
		}

		match codec::Decode::decode(&mut self.input) {
			Ok(r) => {
				self.read += 1;
				Some(r)
			},
			Err(e) => {
				log::error!(
					target: "runtime::storage",
					"Corrupted state at `{:?}`: failed to decode element {} (out of {} in total): {:?}",
					self.input.key,
					self.read,
					self.length,
					e,
				);

				self.read = self.length;
				None
			},
		}
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		let left = (self.length - self.read) as usize;

		(left, Some(left))
	}
}

/// The size of the internal buffer used by [`StorageInput`].
///
/// This internal buffer is used to speed up implementation as reading from the
/// state for every access is too slow.
const STORAGE_INPUT_BUFFER_CAPACITY: usize = 2 * 1024;

/// Implementation of [`codec::Input`] using [`sp_io::storage::read`].
///
/// Keeps an internal buffer with a size of [`STORAGE_INPUT_BUFFER_CAPACITY`]. All read accesses
/// are tried to be served by this buffer. If the buffer doesn't hold enough bytes to fulfill the
/// current read access, the buffer is re-filled from the state. A read request that is bigger than
/// the internal buffer is directly forwarded to the state to reduce the number of reads from the
/// state.
struct StorageInput {
	key: Vec<u8>,
	offset: u32,
	total_length: u32,
	exists: bool,
	buffer: Vec<u8>,
	buffer_pos: usize,
}

impl StorageInput {
	/// Create a new instance of the input.
	///
	/// - `key`: The storage key of the storage item that this input will read.
	fn new(key: Vec<u8>) -> Self {
		let mut buffer = alloc::vec![0; STORAGE_INPUT_BUFFER_CAPACITY];
		unsafe {
			buffer.set_len(buffer.capacity());
		}

		let (total_length, exists) =
			if let Some(total_length) = sp_io::storage::read(&key, &mut buffer, 0) {
				(total_length, true)
			} else {
				(0, false)
			};

		if (total_length as usize) < buffer.len() {
			unsafe {
				buffer.set_len(total_length as usize);
			}
		}

		Self { total_length, offset: buffer.len() as u32, key, exists, buffer, buffer_pos: 0 }
	}

	/// Fill the internal buffer from the state.
	fn fill_buffer(&mut self) -> Result<(), codec::Error> {
		self.buffer.copy_within(self.buffer_pos.., 0);
		let present_bytes = self.buffer.len() - self.buffer_pos;
		self.buffer_pos = 0;

		unsafe {
			self.buffer.set_len(self.buffer.capacity());
		}

		if let Some(length_minus_offset) =
			sp_io::storage::read(&self.key, &mut self.buffer[present_bytes..], self.offset)
		{
			let bytes_read =
				core::cmp::min(length_minus_offset as usize, self.buffer.len() - present_bytes);
			let buffer_len = present_bytes + bytes_read;
			unsafe {
				self.buffer.set_len(buffer_len);
			}

			self.ensure_total_length_did_not_change(length_minus_offset)?;

			self.offset += bytes_read as u32;

			Ok(())
		} else {
			// The value was deleted, let's ensure we don't read anymore.
			self.stop_reading();

			Err("Value doesn't exist in the state?".into())
		}
	}

	/// Returns if the value to read exists in the state.
	fn exists(&self) -> bool {
		self.exists
	}

	/// Reads directly into the given slice `into`.
	///
	/// Should be used when `into.len() > self.buffer.capacity()` to reduce the number of reads from
	/// the state.
	#[inline(never)]
	fn read_big_item(&mut self, into: &mut [u8]) -> Result<(), codec::Error> {
		let num_cached = self.buffer.len() - self.buffer_pos;

		let (out_already_read, mut out_remaining) = into.split_at_mut(num_cached);
		out_already_read.copy_from_slice(&self.buffer[self.buffer_pos..]);

		self.buffer_pos = 0;
		unsafe {
			self.buffer.set_len(0);
		}

		if let Some(length_minus_offset) =
			sp_io::storage::read(&self.key, &mut out_remaining, self.offset)
		{
			if (length_minus_offset as usize) < out_remaining.len() {
				return Err("Not enough data to fill the buffer".into())
			}

			self.ensure_total_length_did_not_change(length_minus_offset)?;

			self.offset += out_remaining.len() as u32;

			Ok(())
		} else {
			// The value was deleted, let's ensure we don't read anymore.
			self.stop_reading();

			Err("Value doesn't exist in the state?".into())
		}
	}

	/// Ensures that the expected total length of the value did not change.
	///
	/// On error ensures that further reading is prohibited.
	fn ensure_total_length_did_not_change(
		&mut self,
		length_minus_offset: u32,
	) -> Result<(), codec::Error> {
		if self.total_length == self.offset + length_minus_offset {
			Ok(())
		} else {
			// The value total length changed, let's ensure we don't read anymore.
			self.stop_reading();

			Err("Storage value changed while it is being read!".into())
		}
	}

	/// Ensure that we are stop reading from this value in the state.
	///
	/// Should be used when there happened an unrecoverable error while reading.
	fn stop_reading(&mut self) {
		self.offset = self.total_length;

		self.buffer_pos = 0;
		unsafe {
			self.buffer.set_len(0);
		}
	}
}

impl codec::Input for StorageInput {
	fn remaining_len(&mut self) -> Result<Option<usize>, codec::Error> {
		Ok(Some(self.total_length.saturating_sub(
			self.offset.saturating_sub((self.buffer.len() - self.buffer_pos) as u32),
		) as usize))
	}

	fn read(&mut self, into: &mut [u8]) -> Result<(), codec::Error> {
		// If there is still data left to be read from the state.
		if self.offset < self.total_length {
			if into.len() > self.buffer.capacity() {
				return self.read_big_item(into)
			} else if self.buffer_pos + into.len() > self.buffer.len() {
				self.fill_buffer()?;
			}
		}

		// Guard against `fill_buffer` not reading enough data or just not having enough data
		// anymore.
		if into.len() + self.buffer_pos > self.buffer.len() {
			return Err("Not enough data to fill the buffer".into())
		}

		let end = self.buffer_pos + into.len();
		into.copy_from_slice(&self.buffer[self.buffer_pos..end]);
		self.buffer_pos = end;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
}
