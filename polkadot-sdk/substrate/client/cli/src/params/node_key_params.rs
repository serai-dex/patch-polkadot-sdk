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

use clap::Args;
use sc_network::config::{ed25519, NodeKeyConfig};
use sc_service::Role;
use sp_core::H256;
use std::{path::PathBuf, str::FromStr};

use crate::{arg_enums::NodeKeyType, error, Error};

/// The file name of the node's Ed25519 secret key inside the chain-specific
/// network config directory, if neither `--node-key` nor `--node-key-file`
/// is specified in combination with `--node-key-type=ed25519`.
pub(crate) const NODE_KEY_ED25519_FILE: &str = "secret_ed25519";

/// Parameters used to create the `NodeKeyConfig`, which determines the keypair
/// used for libp2p networking.
#[derive(Debug, Clone, Args)]
pub struct NodeKeyParams {
	/// Secret key to use for p2p networking.
	///
	/// The value is a string that is parsed according to the choice of
	/// `--node-key-type` as follows:
	///
	///  - `ed25519`: the value is parsed as a hex-encoded Ed25519 32 byte secret key (64 hex
	///    chars)
	///
	/// The value of this option takes precedence over `--node-key-file`.
	///
	/// WARNING: Secrets provided as command-line arguments are easily exposed.
	/// Use of this option should be limited to development and testing. To use
	/// an externally managed secret key, use `--node-key-file` instead.
	#[arg(long, value_name = "KEY")]
	pub node_key: Option<String>,

	/// Crypto primitive to use for p2p networking.
	///
	/// The secret key of the node is obtained as follows:
	///
	/// - If the `--node-key` option is given, the value is parsed as a secret key according to the
	///   type. See the documentation for `--node-key`.
	///
	/// - If the `--node-key-file` option is given, the secret key is read from the specified file.
	///   See the documentation for `--node-key-file`.
	///
	/// - Otherwise, the secret key is read from a file with a predetermined, type-specific name
	///   from the chain-specific network config directory inside the base directory specified by
	///   `--base-dir`. If this file does not exist, it is created with a newly generated secret
	///   key of the chosen type.
	///
	/// The node's secret key determines the corresponding public key and hence the
	/// node's peer ID in the context of libp2p.
	#[arg(long, value_name = "TYPE", value_enum, ignore_case = true, default_value_t = NodeKeyType::Ed25519)]
	pub node_key_type: NodeKeyType,

	/// File from which to read the node's secret key to use for p2p networking.
	///
	/// The contents of the file are parsed according to the choice of `--node-key-type`
	/// as follows:
	///
	/// - `ed25519`: the file must contain an unencoded 32 byte or hex encoded Ed25519 secret key.
	///
	/// If the file does not exist, it is created with a newly generated secret key of
	/// the chosen type.
	#[arg(long, value_name = "FILE")]
	pub node_key_file: Option<PathBuf>,

	/// Forces key generation if node-key-file file does not exist.
	///
	/// This is an unsafe feature for production networks, because as an active authority
	/// other authorities may depend on your node having a stable identity and they might
	/// not being able to reach you if your identity changes after entering the active set.
	///
	/// For minimal node downtime if no custom `node-key-file` argument is provided
	/// the network-key is usually persisted accross nodes restarts,
	/// in the `network` folder from directory provided in `--base-path`
	///
	/// Warning!! If you ever run the node with this argument, make sure
	/// you remove it for the subsequent restarts.
	#[arg(long)]
	pub unsafe_force_node_key_generation: bool,
}

impl NodeKeyParams {
	/// Create a `NodeKeyConfig` from the given `NodeKeyParams` in the context
	/// of an optional network config storage directory.
	pub fn node_key(
		&self,
		net_config_dir: &PathBuf,
		role: Role,
		is_dev: bool,
	) -> error::Result<NodeKeyConfig> {
		Ok(match self.node_key_type {
			NodeKeyType::Ed25519 => {
				let secret = if let Some(node_key) = self.node_key.as_ref() {
					parse_ed25519_secret(node_key)?
				} else {
					let key_path = self
						.node_key_file
						.clone()
						.unwrap_or_else(|| net_config_dir.join(NODE_KEY_ED25519_FILE));
					if !self.unsafe_force_node_key_generation &&
						role.is_authority() &&
						!is_dev && !key_path.exists()
					{
						return Err(Error::NetworkKeyNotFound(key_path))
					}
					sc_network::config::Secret::File(key_path)
				};

				NodeKeyConfig::Ed25519(secret)
			},
		})
	}
}

/// Create an error caused by an invalid node key argument.
fn invalid_node_key(e: impl std::fmt::Display) -> error::Error {
	error::Error::Input(format!("Invalid node key: {}", e))
}

/// Parse a Ed25519 secret key from a hex string into a `sc_network::Secret`.
fn parse_ed25519_secret(hex: &str) -> error::Result<sc_network::config::Ed25519Secret> {
	H256::from_str(hex).map_err(invalid_node_key).and_then(|bytes| {
		ed25519::SecretKey::try_from_bytes(bytes)
			.map(sc_network::config::Secret::Input)
			.map_err(invalid_node_key)
	})
}

#[cfg(test)]
mod tests {
}
