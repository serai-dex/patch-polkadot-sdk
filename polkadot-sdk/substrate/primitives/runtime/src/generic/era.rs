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

//! Generic implementation of an unchecked (pre-verification) extrinsic.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::codec::{Decode, DecodeWithMemTracking, Encode, Error, Input, Output};

/// Era period
pub type Period = u64;

/// Era phase
pub type Phase = u64;

/// An era to describe the longevity of a transaction.
#[derive(DecodeWithMemTracking, PartialEq, Eq, Clone, Copy, sp_core::RuntimeDebug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Era {
	/// The transaction is valid forever. The genesis hash must be present in the signed content.
	Immortal,

	/// Period and phase are encoded:
	/// - The period of validity from the block hash found in the signing material.
	/// - The phase in the period that this transaction's lifetime begins (and, importantly,
	/// implies which block hash is included in the signature material). If the `period` is
	/// greater than 1 << 12, then it will be a factor of the times greater than 1<<12 that
	/// `period` is.
	///
	/// When used on `FRAME`-based runtimes, `period` cannot exceed `BlockHashCount` parameter
	/// of `system` module.
	Mortal(Period, Phase),
}

// E.g. with period == 4:
// 0         10        20        30        40
// 0123456789012345678901234567890123456789012
//              |...|
//    authored -/   \- expiry
// phase = 1
// n = Q(current - phase, period) + phase
impl Era {
	/// Create a new era based on a period (which should be a power of two between 4 and 65536
	/// inclusive) and a block number on which it should start (or, for long periods, be shortly
	/// after the start).
	///
	/// If using `Era` in the context of `FRAME` runtime, make sure that `period`
	/// does not exceed `BlockHashCount` parameter passed to `system` module, since that
	/// prunes old blocks and renders transactions immediately invalid.
	pub fn mortal(period: u64, current: u64) -> Self {
		let period = period.checked_next_power_of_two().unwrap_or(1 << 16).clamp(4, 1 << 16);
		let phase = current % period;
		let quantize_factor = (period >> 12).max(1);
		let quantized_phase = phase / quantize_factor * quantize_factor;

		Self::Mortal(period, quantized_phase)
	}

	/// Create an "immortal" transaction.
	pub fn immortal() -> Self {
		Self::Immortal
	}

	/// `true` if this is an immortal transaction.
	pub fn is_immortal(&self) -> bool {
		matches!(self, Self::Immortal)
	}

	/// Get the block number of the start of the era whose properties this object
	/// describes that `current` belongs to.
	pub fn birth(self, current: u64) -> u64 {
		match self {
			Self::Immortal => 0,
			Self::Mortal(period, phase) => (current.max(phase) - phase) / period * period + phase,
		}
	}

	/// Get the block number of the first block at which the era has ended.
	pub fn death(self, current: u64) -> u64 {
		match self {
			Self::Immortal => u64::MAX,
			Self::Mortal(period, _) => self.birth(current) + period,
		}
	}
}

impl Encode for Era {
	fn encode_to<T: Output + ?Sized>(&self, output: &mut T) {
		match self {
			Self::Immortal => output.push_byte(0),
			Self::Mortal(period, phase) => {
				let quantize_factor = (*period as u64 >> 12).max(1);
				let encoded = (period.trailing_zeros() - 1).clamp(1, 15) as u16 |
					((phase / quantize_factor) << 4) as u16;
				encoded.encode_to(output);
			},
		}
	}
}

impl codec::EncodeLike for Era {}

impl Decode for Era {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let first = input.read_byte()?;
		if first == 0 {
			Ok(Self::Immortal)
		} else {
			let encoded = first as u64 + ((input.read_byte()? as u64) << 8);
			let period = 2 << (encoded % (1 << 4));
			let quantize_factor = (period >> 12).max(1);
			let phase = (encoded >> 4) * quantize_factor;
			if period >= 4 && phase < period {
				Ok(Self::Mortal(period, phase))
			} else {
				Err("Invalid period and phase".into())
			}
		}
	}
}

/// Add Mortal{N}(u8) variants with the given indices, to describe custom encoding.
macro_rules! mortal_variants {
    ($variants:ident, $($index:literal),* ) => {
		$variants
		$(
			.variant(concat!(stringify!(Mortal), stringify!($index)), |v| v
				.index($index)
				.fields(scale_info::build::Fields::unnamed().field(|f| f.ty::<u8>()))
			)
		)*
    }
}

#[cfg(test)]
mod tests {
}
