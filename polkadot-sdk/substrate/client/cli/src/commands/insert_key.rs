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

//! Implementation of the `insert` subcommand

use crate::{
	utils, with_crypto_scheme, CryptoScheme, Error, KeystoreParams, SharedParams, SubstrateCli,
};
use clap::Parser;
use sc_keystore::LocalKeystore;
use sc_service::config::{BasePath, KeystoreConfig};
use sp_core::crypto::{Pair, KeyTypeId, SecretString};
use sp_keystore::KeystorePtr;

/// The `insert` command
#[derive(Debug, Clone, Parser)]
#[command(name = "insert", about = "Insert a key to the keystore of a node.")]
pub struct InsertKeyCmd {
	/// The secret key URI.
	/// If the value is a file, the file content is used as URI.
	/// If not given, you will be prompted for the URI.
	#[arg(long)]
	suri: Option<String>,

	/// Key type, examples: "gran", or "imon".
	#[arg(long)]
	key_type: String,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub shared_params: SharedParams,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub keystore_params: KeystoreParams,

	/// The cryptography scheme that should be used to generate the key out of the given URI.
	#[arg(long, value_name = "SCHEME", value_enum, ignore_case = true)]
	pub scheme: CryptoScheme,
}

impl InsertKeyCmd {
	/// Run the command
	pub fn run<C: SubstrateCli>(&self, cli: &C) -> Result<(), Error> {
		let suri = utils::read_uri(self.suri.as_ref())?;
		let base_path = self
			.shared_params
			.base_path()?
			.unwrap_or_else(|| BasePath::from_project("", "", &C::executable_name()));
		let chain_id = self.shared_params.chain_id(self.shared_params.is_dev());
		let chain_spec = cli.load_spec(&chain_id)?;
		let config_dir = base_path.config_dir(chain_spec.id());

		let (keystore, public) = match self.keystore_params.keystore_config(&config_dir)? {
			KeystoreConfig::Path { path, password } => {
				let public = with_crypto_scheme!(self.scheme, to_vec(&suri, password.clone()))?;
				let keystore: KeystorePtr = LocalKeystore::open(path, password)?.into();
				(keystore, public)
			},
			_ => unreachable!("keystore_config always returns path and password"),
		};

		let key_type =
			KeyTypeId::try_from(self.key_type.as_str()).map_err(|_| Error::KeyTypeInvalid)?;

		keystore
			.insert(key_type, &suri, &public[..])
			.map_err(|_| Error::KeystoreOperation)?;

		Ok(())
	}
}

fn to_vec(uri: &str, pass: Option<SecretString>) -> Result<Vec<u8>, Error> {
	let p = utils::pair_from_suri(uri, pass)?;
	Ok(p.public().to_vec())
}

#[cfg(test)]
mod tests {
}
