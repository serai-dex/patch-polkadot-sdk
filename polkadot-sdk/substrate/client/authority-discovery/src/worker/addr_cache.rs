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

use crate::error::Error;
use log::{info, warn};
use sc_network::{multiaddr::Protocol, Multiaddr};
use sc_network_types::PeerId;
use serde::{Deserialize, Serialize};
use sp_authority_discovery::AuthorityId;
use sp_runtime::DeserializeOwned;
use std::{
	collections::{hash_map::Entry, HashMap, HashSet},
	fs::File,
	io::{self, BufReader, Write},
	path::Path,
};

/// Cache for [`AuthorityId`] -> [`HashSet<Multiaddr>`] and [`PeerId`] -> [`HashSet<AuthorityId>`]
/// mappings.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct AddrCache {
	/// The addresses found in `authority_id_to_addresses` are guaranteed to always match
	/// the peerids found in `peer_id_to_authority_ids`. In other words, these two hashmaps
	/// are similar to a bi-directional map.
	///
	/// Since we may store the mapping across several sessions, a single
	/// `PeerId` might correspond to multiple `AuthorityId`s. However,
	/// it's not expected that a single `AuthorityId` can have multiple `PeerId`s.
	authority_id_to_addresses: HashMap<AuthorityId, HashSet<Multiaddr>>,
	peer_id_to_authority_ids: HashMap<PeerId, HashSet<AuthorityId>>,
}

impl Serialize for AddrCache {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		SerializeAddrCache::from(self.clone()).serialize(serializer)
	}
}

impl<'de> Deserialize<'de> for AddrCache {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		SerializeAddrCache::deserialize(deserializer).map(Into::into)
	}
}

/// A storage and serialization time optimized version of `AddrCache`
/// which contains the bare minimum info to reconstruct the AddrCache. We
/// rely on the fact that the `peer_id_to_authority_ids` can be reconstructed from
/// the `authority_id_to_addresses` field.
///
/// Benchmarks show that this is about 2x faster to serialize and about 4x faster to deserialize
/// compared to the full `AddrCache`.
///
/// Storage wise it is about half the size of the full `AddrCache`.
///
/// This is used to persist the `AddrCache` to disk and load it back.
///
/// AddrCache impl of Serialize and Deserialize "piggybacks" on this struct.
#[derive(Serialize, Deserialize)]
struct SerializeAddrCache {
	authority_id_to_addresses: HashMap<AuthorityId, HashSet<Multiaddr>>,
}

impl From<SerializeAddrCache> for AddrCache {
	fn from(value: SerializeAddrCache) -> Self {
		let mut peer_id_to_authority_ids: HashMap<PeerId, HashSet<AuthorityId>> = HashMap::new();

		for (authority_id, addresses) in &value.authority_id_to_addresses {
			for peer_id in addresses_to_peer_ids(addresses) {
				peer_id_to_authority_ids
					.entry(peer_id)
					.or_insert_with(HashSet::new)
					.insert(authority_id.clone());
			}
		}

		AddrCache {
			authority_id_to_addresses: value.authority_id_to_addresses,
			peer_id_to_authority_ids,
		}
	}
}
impl From<AddrCache> for SerializeAddrCache {
	fn from(value: AddrCache) -> Self {
		Self { authority_id_to_addresses: value.authority_id_to_addresses }
	}
}

fn write_to_file(path: impl AsRef<Path>, contents: &str) -> io::Result<()> {
	let path = path.as_ref();
	let mut file = File::create(path)?;
	file.write_all(contents.as_bytes())?;
	file.flush()?;
	Ok(())
}

impl TryFrom<&Path> for AddrCache {
	type Error = Error;

	fn try_from(path: &Path) -> Result<Self, Self::Error> {
		// Try to load from the cache file if it exists and is valid.
		load_from_file::<AddrCache>(&path).map_err(|e| {
			Error::EncodingDecodingAddrCache(format!(
				"Failed to load AddrCache from file: {}, error: {:?}",
				path.display(),
				e
			))
		})
	}
}
impl AddrCache {
	pub fn new() -> Self {
		AddrCache::default()
	}

	fn serialize(&self) -> Option<String> {
		serde_json::to_string_pretty(self).inspect_err(|e| {
			warn!(target: super::LOG_TARGET, "Failed to serialize AddrCache to JSON: {} => skip persisting it.", e);
		}).ok()
	}

	fn persist(path: impl AsRef<Path>, serialized_cache: String) {
		match write_to_file(path.as_ref(), &serialized_cache) {
			Err(err) => {
				warn!(target: super::LOG_TARGET, "Failed to persist AddrCache on disk at path: {}, error: {}", path.as_ref().display(), err);
			},
			Ok(_) => {
				info!(target: super::LOG_TARGET, "Successfully persisted AddrCache on disk");
			},
		}
	}

	pub fn serialize_and_persist(&self, path: impl AsRef<Path>) {
		let Some(serialized) = self.serialize() else { return };
		Self::persist(path, serialized);
	}

	/// Inserts the given [`AuthorityId`] and [`Vec<Multiaddr>`] pair for future lookups by
	/// [`AuthorityId`] or [`PeerId`].
	pub fn insert(&mut self, authority_id: AuthorityId, addresses: Vec<Multiaddr>) {
		let addresses = addresses.into_iter().collect::<HashSet<_>>();
		let peer_ids = addresses_to_peer_ids(&addresses);

		if peer_ids.is_empty() {
			log::debug!(
				target: super::LOG_TARGET,
				"Authority({:?}) provides no addresses or addresses without peer ids. Adresses: {:?}",
				authority_id,
				addresses,
			);
			return;
		} else if peer_ids.len() > 1 {
			log::warn!(
				target: super::LOG_TARGET,
				"Authority({:?}) can be reached through multiple peer ids: {:?}",
				authority_id,
				peer_ids
			);
		}

		log::debug!(
			target: super::LOG_TARGET,
			"Found addresses for authority {authority_id:?}: {addresses:?}",
		);

		let old_addresses = self.authority_id_to_addresses.insert(authority_id.clone(), addresses);
		let old_peer_ids = addresses_to_peer_ids(&old_addresses.unwrap_or_default());

		// Add the new peer ids
		peer_ids.difference(&old_peer_ids).for_each(|new_peer_id| {
			self.peer_id_to_authority_ids
				.entry(*new_peer_id)
				.or_default()
				.insert(authority_id.clone());
		});

		// Remove the old peer ids
		self.remove_authority_id_from_peer_ids(&authority_id, old_peer_ids.difference(&peer_ids));
	}

	/// Remove the given `authority_id` from the `peer_id` to `authority_ids` mapping.
	///
	/// If a `peer_id` doesn't have any `authority_id` assigned anymore, it is removed.
	fn remove_authority_id_from_peer_ids<'a>(
		&mut self,
		authority_id: &AuthorityId,
		peer_ids: impl Iterator<Item = &'a PeerId>,
	) {
		peer_ids.for_each(|peer_id| {
			if let Entry::Occupied(mut e) = self.peer_id_to_authority_ids.entry(*peer_id) {
				e.get_mut().remove(authority_id);

				// If there are no more entries, remove the peer id.
				if e.get().is_empty() {
					e.remove();
				}
			}
		})
	}

	/// Returns the number of authority IDs in the cache.
	pub fn num_authority_ids(&self) -> usize {
		self.authority_id_to_addresses.len()
	}

	/// Returns the addresses for the given [`AuthorityId`].
	pub fn get_addresses_by_authority_id(
		&self,
		authority_id: &AuthorityId,
	) -> Option<&HashSet<Multiaddr>> {
		self.authority_id_to_addresses.get(authority_id)
	}

	/// Returns the [`AuthorityId`]s for the given [`PeerId`].
	///
	/// As the authority id can change between sessions, one [`PeerId`] can be mapped to
	/// multiple authority ids.
	pub fn get_authority_ids_by_peer_id(&self, peer_id: &PeerId) -> Option<&HashSet<AuthorityId>> {
		self.peer_id_to_authority_ids.get(peer_id)
	}

	/// Removes all [`PeerId`]s and [`Multiaddr`]s from the cache that are not related to the given
	/// [`AuthorityId`]s.
	pub fn retain_ids(&mut self, authority_ids: &[AuthorityId]) {
		// The below logic could be replaced by `BtreeMap::drain_filter` once it stabilized.
		let authority_ids_to_remove = self
			.authority_id_to_addresses
			.iter()
			.filter(|(id, _addresses)| !authority_ids.contains(id))
			.map(|entry| entry.0)
			.cloned()
			.collect::<Vec<AuthorityId>>();

		for authority_id_to_remove in authority_ids_to_remove {
			// Remove other entries from `self.authority_id_to_addresses`.
			let addresses = if let Some(addresses) =
				self.authority_id_to_addresses.remove(&authority_id_to_remove)
			{
				addresses
			} else {
				continue;
			};

			self.remove_authority_id_from_peer_ids(
				&authority_id_to_remove,
				addresses_to_peer_ids(&addresses).iter(),
			);
		}
	}
}

fn peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
	addr.iter().last().and_then(|protocol| {
		if let Protocol::P2p(multihash) = protocol {
			PeerId::from_multihash(multihash).ok()
		} else {
			None
		}
	})
}

fn addresses_to_peer_ids(addresses: &HashSet<Multiaddr>) -> HashSet<PeerId> {
	addresses.iter().filter_map(peer_id_from_multiaddr).collect::<HashSet<_>>()
}

fn load_from_file<T: DeserializeOwned>(path: impl AsRef<Path>) -> io::Result<T> {
	let file = File::open(path)?;
	let reader = BufReader::new(file);

	serde_json::from_reader(reader).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
}
