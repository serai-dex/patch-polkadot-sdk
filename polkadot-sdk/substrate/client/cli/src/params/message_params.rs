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

//! Params to configure how a message should be passed into a command.

use crate::error::Error;
use array_bytes::{hex2bytes, hex_bytes2hex_str};
use clap::Args;
use std::io::BufRead;

/// Params to configure how a message should be passed into a command.
#[derive(Debug, Clone, Args)]
pub struct MessageParams {
	/// Message to process. Will be read from STDIN otherwise.
	/// The message is assumed to be raw bytes per default. Use `--hex` for hex input. Can
	/// optionally be prefixed with `0x` in the hex case.
	#[arg(long)]
	message: Option<String>,

	/// The message is hex-encoded data.
	#[arg(long)]
	hex: bool,
}

impl MessageParams {
	/// Produces the message by either using its immediate value or reading from stdin.
	///
	/// This function should only be called once and the result cached.
	pub(crate) fn message_from<F, R>(&self, create_reader: F) -> Result<Vec<u8>, Error>
	where
		R: BufRead,
		F: FnOnce() -> R,
	{
		let raw = match &self.message {
			Some(raw) => raw.as_bytes().to_vec(),
			None => {
				let mut raw = vec![];
				create_reader().read_to_end(&mut raw)?;
				raw
			},
		};
		if self.hex {
			hex2bytes(hex_bytes2hex_str(&raw)?).map_err(Into::into)
		} else {
			Ok(raw)
		}
	}
}

#[cfg(test)]
mod tests {
}
