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

//! Implementation of the `inspect` subcommand

use crate::{
	utils::{self, print_from_public, print_from_uri},
	with_crypto_scheme, CryptoSchemeFlag, Error, KeystoreParams, NetworkSchemeFlag, OutputTypeFlag,
};
use clap::Parser;
use sp_core::crypto::{Pair, ExposeSecret, SecretString, SecretUri, Ss58Codec};
use std::str::FromStr;

/// The `inspect` command
#[derive(Debug, Parser)]
#[command(
	name = "inspect",
	about = "Gets a public key and a SS58 address from the provided Secret URI"
)]
pub struct InspectKeyCmd {
	/// A Key URI to be inspected. May be a secret seed, secret URI
	/// (with derivation paths and password), SS58, public URI or a hex encoded public key.
	/// If it is a hex encoded public key, `--public` needs to be given as argument.
	/// If the given value is a file, the file content will be used
	/// as URI.
	/// If omitted, you will be prompted for the URI.
	uri: Option<String>,

	/// Is the given `uri` a hex encoded public key?
	#[arg(long)]
	public: bool,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub keystore_params: KeystoreParams,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub network_scheme: NetworkSchemeFlag,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub output_scheme: OutputTypeFlag,

	#[allow(missing_docs)]
	#[clap(flatten)]
	pub crypto_scheme: CryptoSchemeFlag,

	/// Expect that `--uri` has the given public key/account-id.
	/// If `--uri` has any derivations, the public key is checked against the base `uri`, i.e. the
	/// `uri` without any derivation applied. However, if `uri` has a password or there is one
	/// given by `--password`, it will be used to decrypt `uri` before comparing the public
	/// key/account-id.
	/// If there is no derivation in `--uri`, the public key will be checked against the public key
	/// of `--uri` directly.
	#[arg(long, conflicts_with = "public")]
	pub expect_public: Option<String>,
}

impl InspectKeyCmd {
	/// Run the command
	pub fn run(&self) -> Result<(), Error> {
		let uri = utils::read_uri(self.uri.as_ref())?;
		let password = self.keystore_params.read_password()?;

		if self.public {
			with_crypto_scheme!(
				self.crypto_scheme.scheme,
				print_from_public(
					&uri,
					self.network_scheme.network,
					self.output_scheme.output_type,
				)
			)?;
		} else {
			if let Some(ref expect_public) = self.expect_public {
				with_crypto_scheme!(
					self.crypto_scheme.scheme,
					expect_public_from_phrase(expect_public, &uri, password.as_ref())
				)?;
			}

			with_crypto_scheme!(
				self.crypto_scheme.scheme,
				print_from_uri(
					&uri,
					password,
					self.network_scheme.network,
					self.output_scheme.output_type,
				)
			);
		}

		Ok(())
	}
}

/// Checks that `expect_public` is the public key of `suri`.
///
/// If `suri` has any derivations, `expect_public` is checked against the public key of the "bare"
/// `suri`, i.e. without any derivations.
///
/// Returns an error if the public key does not match.
fn expect_public_from_phrase(
	expect_public: &str,
	suri: &str,
	password: Option<&SecretString>,
) -> Result<(), Error> {
	let secret_uri = SecretUri::from_str(suri).map_err(|e| format!("{:?}", e))?;
	let expected_public = if let Some(public) = expect_public.strip_prefix("0x") {
		let hex_public = array_bytes::hex2bytes(public)
			.map_err(|_| format!("Invalid expected public key hex: `{}`", expect_public))?;
		sp_core::sr25519::Public::try_from(hex_public.as_slice())
			.map_err(|_| format!("Invalid expected public key: `{}`", expect_public))?
	} else {
		sp_core::sr25519::Public::from_string_with_version(expect_public)
			.map_err(|_| format!("Invalid expected account id: `{}`", expect_public))?
			.0
	};

	let pair = sp_core::sr25519::Pair::from_string_with_seed(
		secret_uri.phrase.expose_secret().as_str(),
		password
			.or_else(|| secret_uri.password.as_ref())
			.map(|p| p.expose_secret().as_str()),
	)
	.map_err(|_| format!("Invalid secret uri: {}", suri))?
	.0;

	if pair.public() == expected_public {
		Ok(())
	} else {
		Err(format!("Expected public ({}) key does not match.", expect_public).into())
	}
}

#[cfg(test)]
mod tests {
}
