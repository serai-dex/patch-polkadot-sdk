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

//! Helper macro allowing to construct JSON representation of partially initialized structs.

use serde_json::Value;
extern crate alloc;
use alloc::{borrow::Cow, format, string::String};

/// Represents the initialization method of a field within a struct.
///
/// This enum provides information about how it was initialized.
///
/// Intended to be used in `build_struct_json_patch` macro.
#[derive(Debug)]
pub enum InitilizationType {
	/// The field was partially initialized (e.g., specific fields within the struct were set
	/// manually).
	Partial,
	/// The field was fully initialized (e.g., using `new()` or `default()` like methods
	Full,
}

/// This struct provides information about how the struct field was initialized and the field name
/// (as a `&str`).
///
/// Intended to be used in `build_struct_json_patch` macro.
#[derive(Debug)]
pub struct InitializedField<'a>(InitilizationType, Cow<'a, str>);

impl<'a> InitializedField<'a> {
	/// Returns a name of the field.
	pub fn get_name(&'a self) -> &'a str {
		&self.1
	}

	/// Injects a prefix to the field name.
	pub fn add_prefix(&mut self, prefix: &str) {
		self.1 = format!("{prefix}.{}", self.1).into()
	}

	/// Creates new partial field instiance.
	pub fn partial(s: &'a str) -> Self {
		Self(InitilizationType::Partial, s.into())
	}

	/// Creates new full field instiance.
	pub fn full(s: &'a str) -> Self {
		Self(InitilizationType::Full, s.into())
	}
}

impl PartialEq<String> for InitializedField<'_> {
	fn eq(&self, other: &String) -> bool {
		#[inline]
		/// We need to respect the `camelCase` naming for field names. This means that
		/// `"camelCaseKey"` should be considered equal to `"camel_case_key"`. This
		/// function implements this comparison.
		fn compare_keys(ident_chars: core::str::Chars, camel_chars: core::str::Chars) -> bool {
			ident_chars
				.filter(|c| *c != '_')
				.map(|c| c.to_ascii_uppercase())
				.eq(camel_chars.map(|c| c.to_ascii_uppercase()))
		}
		*self.1 == *other || compare_keys(self.1.chars(), other.chars())
	}
}

impl<'a> From<(InitilizationType, &'a str)> for InitializedField<'a> {
	fn from(value: (InitilizationType, &'a str)) -> Self {
		match value.0 {
			InitilizationType::Full => InitializedField::full(value.1),
			InitilizationType::Partial => InitializedField::partial(value.1),
		}
	}
}

/// Recursively removes keys from provided `json_value` object, retaining only specified keys.
///
/// This function modifies the provided `json_value` in-place, keeping only the keys listed in
/// `keys_to_retain`. The keys are matched recursively by combining the current key with
/// the `current_root`, allowing for nested field retention.
///
/// Keys marked as `Full`, are retained as-is. For keys marked as `Partial`, the
/// function recurses into nested objects to retain matching subfields.
///
/// Function respects the `camelCase` serde_json attribute for structures. This means that
/// `"camelCaseKey"` key will be retained in JSON blob if `"camel_case_key"` exists in
/// `keys_to_retain`.
///
/// Intended to be used from `build_struct_json_patch` macro.
pub fn retain_initialized_fields(
	json_value: &mut Value,
	keys_to_retain: &[InitializedField],
	current_root: String,
) {
	if let serde_json::Value::Object(ref mut map) = json_value {
		map.retain(|key, value| {
			let current_key =
				if current_root.is_empty() { key.clone() } else { format!("{current_root}.{key}") };
			match keys_to_retain.iter().find(|key| **key == current_key) {
				Some(InitializedField(InitilizationType::Full, _)) => true,
				Some(InitializedField(InitilizationType::Partial, _)) => {
					retain_initialized_fields(value, keys_to_retain, current_key.clone());
					true
				},
				None => false,
			}
		})
	}
}

/// Creates a JSON patch for given `struct_type`, supporting recursive field initialization.
///
/// This macro creates a default `struct_type`, initializing specified fields (which can be nested)
/// with the provided values. Any fields not explicitly given are initialized with their default
/// values. The macro then serializes the fully initialized structure into a JSON blob, retaining
/// only the fields that were explicitly provided, either partially or fully initialized.
///
/// Using this macro prevents errors from manually creating JSON objects, such as typos or
/// inconsistencies with the `struct_type` structure, by relying on the actual
/// struct definition. This ensures the generated JSON is valid and reflects any future changes
/// to the structure.
///
/// # Example
///
/// ```rust
/// use frame_support::build_struct_json_patch;
/// #[derive(Default, serde::Serialize, serde::Deserialize)]
/// #[serde(rename_all = "camelCase")]
/// struct RuntimeGenesisConfig {
///     a_field: u32,
///     b_field: B,
///     c_field: u32,
/// }
///
/// #[derive(Default, serde::Serialize, serde::Deserialize)]
/// #[serde(rename_all = "camelCase")]
/// struct B {
/// 	i_field: u32,
/// 	j_field: u32,
/// }
/// impl B {
/// 	fn new() -> Self {
/// 		Self { i_field: 0, j_field: 2 }
/// 	}
/// }
///
/// assert_eq!(
/// 	build_struct_json_patch! ( RuntimeGenesisConfig {
/// 		a_field: 66,
/// 	}),
/// 	serde_json::json!({
/// 			"aField": 66,
/// 	})
/// );
///
/// assert_eq!(
/// 	build_struct_json_patch! ( RuntimeGenesisConfig {
/// 		//"partial" initialization of `b_field`
/// 		b_field: B {
/// 			i_field: 2,
/// 		}
/// 	}),
/// 	serde_json::json!({
/// 		"bField": {"iField": 2}
/// 	})
/// );
///
/// assert_eq!(
/// 	build_struct_json_patch! ( RuntimeGenesisConfig {
/// 		a_field: 66,
/// 		//"full" initialization of `b_field`
/// 		b_field: B::new()
/// 	}),
/// 	serde_json::json!({
/// 		"aField": 66,
/// 		"bField": {"iField": 0, "jField": 2}
/// 	})
/// );
/// ```
///
/// In this example:
/// ```ignore
/// 	build_struct_json_patch! ( RuntimeGenesisConfig {
/// 		b_field: B {
/// 			i_field: 2,
/// 		}
/// 	}),
/// ```
/// `b_field` is partially initialized, it will be expanded to:
/// ```ignore
/// RuntimeGenesisConfig {
/// 		b_field {
/// 			i_field: 2,
/// 			..Default::default()
/// 		},
/// 		..Default::default()
/// }
/// ```
/// While all other fields are initialized with default values. The macro serializes this, retaining
/// only the provided fields.
#[macro_export]
macro_rules! build_struct_json_patch {
	(
		$($struct_type:ident)::+ { $($body:tt)* }
	) => {
		{
			let mut __keys = $crate::__private::Vec::<$crate::generate_genesis_config::InitializedField>::default();
			#[allow(clippy::needless_update)]
			let __struct_instance = $crate::build_struct_json_patch!($($struct_type)::+, __keys @  { $($body)* }).0;
			let mut __json_value =
				$crate::__private::serde_json::to_value(__struct_instance).expect("serialization to json should work. qed");
			$crate::generate_genesis_config::retain_initialized_fields(&mut __json_value, &__keys, Default::default());
			__json_value
		}
	};
	($($struct_type:ident)::+, $all_keys:ident @ { $($body:tt)* }) => {
		{
			let __value = $crate::build_struct_json_patch!($($struct_type)::+, $all_keys @ $($body)*);
			(
				$($struct_type)::+ { ..__value.0 },
				__value.1
			)
		}
	};
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident:  $($type:ident)::+ { $($body:tt)* } ) => {
		(
			$($struct_type)::+ {
				$key: {
					let mut __inner_keys =
						$crate::__private::Vec::<$crate::generate_genesis_config::InitializedField>::default();
					let __value = $crate::build_struct_json_patch!($($type)::+, __inner_keys @ { $($body)* });
					for i in __inner_keys.iter_mut() {
						i.add_prefix(stringify!($key));
					};
					$all_keys.push((__value.1,stringify!($key)).into());
					$all_keys.extend(__inner_keys);
					__value.0
				},
				..Default::default()
			},
			$crate::generate_genesis_config::InitilizationType::Partial
		)
	};
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident:  $($type:ident)::+ { $($body:tt)* },  $($tail:tt)*) => {
		{
			let mut __initialization_type;
			(
				$($struct_type)::+ {
					$key : {
						let mut __inner_keys =
							$crate::__private::Vec::<$crate::generate_genesis_config::InitializedField>::default();
						let __value = $crate::build_struct_json_patch!($($type)::+, __inner_keys @ { $($body)* });
						$all_keys.push((__value.1,stringify!($key)).into());

						for i in __inner_keys.iter_mut() {
							i.add_prefix(stringify!($key));
						};
						$all_keys.extend(__inner_keys);
						__value.0
					},
					.. {
						let (__value, __tmp) =
							$crate::build_struct_json_patch!($($struct_type)::+, $all_keys @ $($tail)*);
						__initialization_type = __tmp;
						__value
					}
				},
				__initialization_type
			)
		}
	};
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident: $value:expr, $($tail:tt)* ) => {
		{
			let mut __initialization_type;
			(
				$($struct_type)::+ {
					$key: {
						$all_keys.push($crate::generate_genesis_config::InitializedField::full(
							stringify!($key))
						);
						$value
					},
					.. {
						let (__value, __tmp) =
							$crate::build_struct_json_patch!($($struct_type)::+, $all_keys @ $($tail)*);
						__initialization_type = __tmp;
						__value
					}
				},
				__initialization_type
			)
		}
	};
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident: $value:expr ) => {
		(
			$($struct_type)::+ {
				$key: {
					$all_keys.push($crate::generate_genesis_config::InitializedField::full(stringify!($key)));
					$value
				},
				..Default::default()
			},
			$crate::generate_genesis_config::InitilizationType::Partial
		)
	};
	// field init shorthand
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident, $($tail:tt)* ) => {
		{
			let __update = $crate::build_struct_json_patch!($($struct_type)::+, $all_keys @ $($tail)*);
			(
				$($struct_type)::+ {
					$key: {
						$all_keys.push($crate::generate_genesis_config::InitializedField::full(
							stringify!($key))
						);
						$key
					},
					..__update.0
				},
				__update.1
			)
		}
	};
	($($struct_type:ident)::+, $all_keys:ident @ $key:ident ) => {
		(
			$($struct_type)::+ {
				$key: {
					$all_keys.push($crate::generate_genesis_config::InitializedField::full(stringify!($key)));
					$key
				},
				..Default::default()
			},
			$crate::generate_genesis_config::InitilizationType::Partial
		)
	};
	// update struct
	($($struct_type:ident)::+, $all_keys:ident @ ..$update:expr ) => {
		(
			$($struct_type)::+ {
				..$update
			},
			$crate::generate_genesis_config::InitilizationType::Full
		)
	};
	($($struct_type:ident)::+, $all_keys:ident  @ $(,)?) => {
		(
			$($struct_type)::+ {
				..Default::default()
			},
			$crate::generate_genesis_config::InitilizationType::Partial
		)
	};
}

#[cfg(test)]
mod test {
}

#[cfg(test)]
mod retain_keys_test {
	use super::*;
	use serde_json::json;

	macro_rules! check_initialized_field_eq_cc(
		( $s:literal ) => {
			let field = InitializedField::full($s);
			let cc = inflector::cases::camelcase::to_camel_case($s);
			assert_eq!(field,cc);
		} ;
		( &[ $f:literal $(, $r:literal)* ]) => {
			let field = InitializedField::full(
				concat!( $f $(,".",$r)+ )
			);
			let cc = [ $f $(,$r)+  ].into_iter()
				.map(|s| inflector::cases::camelcase::to_camel_case(s))
				.collect::<Vec<_>>()
				.join(".");
			assert_eq!(field,cc);
		} ;
	);

	#[test]
	fn test_initialized_field_eq_cc_string() {
		check_initialized_field_eq_cc!("a_");
		check_initialized_field_eq_cc!("abc");
		check_initialized_field_eq_cc!("aBc");
		check_initialized_field_eq_cc!("aBC");
		check_initialized_field_eq_cc!("ABC");
		check_initialized_field_eq_cc!("2abs");
		check_initialized_field_eq_cc!("2Abs");
		check_initialized_field_eq_cc!("2ABs");
		check_initialized_field_eq_cc!("2aBs");
		check_initialized_field_eq_cc!("AlreadyCamelCase");
		check_initialized_field_eq_cc!("alreadyCamelCase");
		check_initialized_field_eq_cc!("C");
		check_initialized_field_eq_cc!("1a");
		check_initialized_field_eq_cc!("_1a");
		check_initialized_field_eq_cc!("a_b");
		check_initialized_field_eq_cc!("_a_b");
		check_initialized_field_eq_cc!("a___b");
		check_initialized_field_eq_cc!("__a_b");
		check_initialized_field_eq_cc!("_a___b_C");
		check_initialized_field_eq_cc!("__A___B_C");
		check_initialized_field_eq_cc!(&["a_b", "b_c"]);
		check_initialized_field_eq_cc!(&["al_pha", "_a___b_C"]);
		check_initialized_field_eq_cc!(&["al_pha_", "_a___b_C"]);
		check_initialized_field_eq_cc!(&["first_field", "al_pha_", "_a___b_C"]);
		check_initialized_field_eq_cc!(&["al_pha_", "__2nd_field", "_a___b_C"]);
		check_initialized_field_eq_cc!(&["al_pha_", "__2nd3and_field", "_a___b_C"]);
		check_initialized_field_eq_cc!(&["_a1", "_a2", "_a3_"]);
	}

	#[test]
	fn test01() {
		let mut v = json!({
			"a":1
		});
		let e = v.clone();
		retain_initialized_fields(&mut v, &[InitializedField::full("a")], String::default());
		assert_eq!(e, v);
	}

	#[test]
	fn test02() {
		let mut v = json!({
			"a":1
		});
		retain_initialized_fields(&mut v, &[InitializedField::full("b")], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test03() {
		let mut v = json!({});
		retain_initialized_fields(&mut v, &[], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test04() {
		let mut v = json!({});
		retain_initialized_fields(&mut v, &[InitializedField::full("b")], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test05() {
		let mut v = json!({
			"a":1
		});
		retain_initialized_fields(&mut v, &[], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test06() {
		let mut v = json!({
			"a": {
				"b":1,
				"c":2
			}
		});
		retain_initialized_fields(&mut v, &[], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test07() {
		let mut v = json!({
			"a": {
				"b":1,
				"c":2
			}
		});
		retain_initialized_fields(&mut v, &[InitializedField::full("a.b")], String::default());
		assert_eq!(Value::Object(Default::default()), v);
	}

	#[test]
	fn test08() {
		let mut v = json!({
			"a": {
				"b":1,
				"c":2
			}
		});
		let e = json!({
			"a": {
				"b":1,
			}
		});
		retain_initialized_fields(
			&mut v,
			&[InitializedField::partial("a"), InitializedField::full("a.b")],
			String::default(),
		);
		assert_eq!(e, v);
	}

	#[test]
	fn test09() {
		let mut v = json!({
			"a": {
				"b":1,
				"c":2
			}
		});
		let e = json!({
			"a": {
				"b":1,
				"c":2,
			}
		});
		retain_initialized_fields(&mut v, &[InitializedField::full("a")], String::default());
		assert_eq!(e, v);
	}
}
