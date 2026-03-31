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

use crate::{biguint::BigUint, helpers_128bit, Rounding};
use core::cmp::Ordering;
use num_traits::{Bounded, One, Zero};

/// A wrapper for any rational number with infinitely large numerator and denominator.
///
/// This type exists to facilitate `cmp` operation
/// on values like `a/b < c/d` where `a, b, c, d` are all `BigUint`.
#[derive(Clone, Default, Eq)]
pub struct RationalInfinite(BigUint, BigUint);

impl RationalInfinite {
	/// Return the numerator reference.
	pub fn n(&self) -> &BigUint {
		&self.0
	}

	/// Return the denominator reference.
	pub fn d(&self) -> &BigUint {
		&self.1
	}

	/// Build from a raw `n/d`.
	pub fn from(n: BigUint, d: BigUint) -> Self {
		Self(n, d.max(BigUint::one()))
	}

	/// Zero.
	pub fn zero() -> Self {
		Self(BigUint::zero(), BigUint::one())
	}

	/// One.
	pub fn one() -> Self {
		Self(BigUint::one(), BigUint::one())
	}
}

impl PartialOrd for RationalInfinite {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for RationalInfinite {
	fn cmp(&self, other: &Self) -> Ordering {
		// handle some edge cases.
		if self.d() == other.d() {
			self.n().cmp(other.n())
		} else if self.d().is_zero() {
			Ordering::Greater
		} else if other.d().is_zero() {
			Ordering::Less
		} else {
			// (a/b) cmp (c/d) => (a*d) cmp (c*b)
			self.n().clone().mul(other.d()).cmp(&other.n().clone().mul(self.d()))
		}
	}
}

impl PartialEq for RationalInfinite {
	fn eq(&self, other: &Self) -> bool {
		self.cmp(other) == Ordering::Equal
	}
}

impl From<Rational128> for RationalInfinite {
	fn from(t: Rational128) -> Self {
		Self(t.0.into(), t.1.into())
	}
}

/// A wrapper for any rational number with a 128 bit numerator and denominator.
#[derive(Clone, Copy, Default, Eq)]
pub struct Rational128(u128, u128);

#[cfg(feature = "std")]
impl core::fmt::Debug for Rational128 {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "Rational128({} / {} ≈ {:.8})", self.0, self.1, self.0 as f64 / self.1 as f64)
	}
}

#[cfg(not(feature = "std"))]
impl core::fmt::Debug for Rational128 {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "Rational128({} / {})", self.0, self.1)
	}
}

impl Rational128 {
	/// Zero.
	pub fn zero() -> Self {
		Self(0, 1)
	}

	/// One
	pub fn one() -> Self {
		Self(1, 1)
	}

	/// If it is zero or not
	pub fn is_zero(&self) -> bool {
		self.0.is_zero()
	}

	/// Build from a raw `n/d`.
	pub fn from(n: u128, d: u128) -> Self {
		Self(n, d.max(1))
	}

	/// Build from a raw `n/d`. This could lead to / 0 if not properly handled.
	pub fn from_unchecked(n: u128, d: u128) -> Self {
		Self(n, d)
	}

	/// Return the numerator.
	pub fn n(&self) -> u128 {
		self.0
	}

	/// Return the denominator.
	pub fn d(&self) -> u128 {
		self.1
	}

	/// Convert `self` to a similar rational number where denominator is the given `den`.
	//
	/// This only returns if the result is accurate. `None` is returned if the result cannot be
	/// accurately calculated.
	pub fn to_den(self, den: u128) -> Option<Self> {
		if den == self.1 {
			Some(self)
		} else {
			helpers_128bit::multiply_by_rational_with_rounding(
				self.0,
				den,
				self.1,
				Rounding::NearestPrefDown,
			)
			.map(|n| Self(n, den))
		}
	}

	/// Get the least common divisor of `self` and `other`.
	///
	/// This only returns if the result is accurate. `None` is returned if the result cannot be
	/// accurately calculated.
	pub fn lcm(&self, other: &Self) -> Option<u128> {
		// this should be tested better: two large numbers that are almost the same.
		if self.1 == other.1 {
			return Some(self.1);
		}
		let g = helpers_128bit::gcd(self.1, other.1);
		helpers_128bit::multiply_by_rational_with_rounding(
			self.1,
			other.1,
			g,
			Rounding::NearestPrefDown,
		)
	}

	/// A saturating add that assumes `self` and `other` have the same denominator.
	pub fn lazy_saturating_add(self, other: Self) -> Self {
		if other.is_zero() {
			self
		} else {
			Self(self.0.saturating_add(other.0), self.1)
		}
	}

	/// A saturating subtraction that assumes `self` and `other` have the same denominator.
	pub fn lazy_saturating_sub(self, other: Self) -> Self {
		if other.is_zero() {
			self
		} else {
			Self(self.0.saturating_sub(other.0), self.1)
		}
	}

	/// Addition. Simply tries to unify the denominators and add the numerators.
	///
	/// Overflow might happen during any of the steps. Error is returned in such cases.
	pub fn checked_add(self, other: Self) -> Result<Self, &'static str> {
		let lcm = self.lcm(&other).ok_or(0).map_err(|_| "failed to scale to denominator")?;
		let self_scaled =
			self.to_den(lcm).ok_or(0).map_err(|_| "failed to scale to denominator")?;
		let other_scaled =
			other.to_den(lcm).ok_or(0).map_err(|_| "failed to scale to denominator")?;
		let n = self_scaled
			.0
			.checked_add(other_scaled.0)
			.ok_or("overflow while adding numerators")?;
		Ok(Self(n, self_scaled.1))
	}

	/// Subtraction. Simply tries to unify the denominators and subtract the numerators.
	///
	/// Overflow might happen during any of the steps. None is returned in such cases.
	pub fn checked_sub(self, other: Self) -> Result<Self, &'static str> {
		let lcm = self.lcm(&other).ok_or(0).map_err(|_| "failed to scale to denominator")?;
		let self_scaled =
			self.to_den(lcm).ok_or(0).map_err(|_| "failed to scale to denominator")?;
		let other_scaled =
			other.to_den(lcm).ok_or(0).map_err(|_| "failed to scale to denominator")?;

		let n = self_scaled
			.0
			.checked_sub(other_scaled.0)
			.ok_or("overflow while subtracting numerators")?;
		Ok(Self(n, self_scaled.1))
	}
}

impl Bounded for Rational128 {
	fn min_value() -> Self {
		Self(0, 1)
	}

	fn max_value() -> Self {
		Self(Bounded::max_value(), 1)
	}
}

impl<T: Into<u128>> From<T> for Rational128 {
	fn from(t: T) -> Self {
		Self::from(t.into(), 1)
	}
}

impl PartialOrd for Rational128 {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Rational128 {
	fn cmp(&self, other: &Self) -> Ordering {
		// handle some edge cases.
		if self.1 == other.1 {
			self.0.cmp(&other.0)
		} else if self.1.is_zero() {
			Ordering::Greater
		} else if other.1.is_zero() {
			Ordering::Less
		} else {
			// Don't even compute gcd.
			let self_n = helpers_128bit::to_big_uint(self.0) * helpers_128bit::to_big_uint(other.1);
			let other_n =
				helpers_128bit::to_big_uint(other.0) * helpers_128bit::to_big_uint(self.1);
			self_n.cmp(&other_n)
		}
	}
}

impl PartialEq for Rational128 {
	fn eq(&self, other: &Self) -> bool {
		// handle some edge cases.
		if self.1 == other.1 {
			self.0.eq(&other.0)
		} else {
			let self_n = helpers_128bit::to_big_uint(self.0) * helpers_128bit::to_big_uint(other.1);
			let other_n =
				helpers_128bit::to_big_uint(other.0) * helpers_128bit::to_big_uint(self.1);
			self_n.eq(&other_n)
		}
	}
}

pub trait MultiplyRational: Sized {
	fn multiply_rational(self, n: Self, d: Self, r: Rounding) -> Option<Self>;
}

macro_rules! impl_rrm {
	($ulow:ty, $uhi:ty) => {
		impl MultiplyRational for $ulow {
			fn multiply_rational(self, n: Self, d: Self, r: Rounding) -> Option<Self> {
				if d.is_zero() {
					return None;
				}

				let sn = (self as $uhi) * (n as $uhi);
				let mut result = sn / (d as $uhi);
				let remainder = (sn % (d as $uhi)) as $ulow;
				if match r {
					Rounding::Up => remainder > 0,
					// cannot be `(d + 1) / 2` since `d` might be `max_value` and overflow.
					Rounding::NearestPrefUp => remainder >= d / 2 + d % 2,
					Rounding::NearestPrefDown => remainder > d / 2,
					Rounding::Down => false,
				} {
					result = match result.checked_add(1) {
						Some(v) => v,
						None => return None,
					};
				}
				if result > (<$ulow>::max_value() as $uhi) {
					None
				} else {
					Some(result as $ulow)
				}
			}
		}
	};
}

impl_rrm!(u8, u16);
impl_rrm!(u16, u32);
impl_rrm!(u32, u64);
impl_rrm!(u64, u128);

impl MultiplyRational for u128 {
	fn multiply_rational(self, n: Self, d: Self, r: Rounding) -> Option<Self> {
		crate::helpers_128bit::multiply_by_rational_with_rounding(self, n, d, r)
	}
}

#[cfg(test)]
mod tests {
}
