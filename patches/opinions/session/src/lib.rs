#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
//! A minified `pallet-session`.

pub use pallet_session_original::ShouldEndSession;

/// Get the current session for Substrate's consensus.
pub trait GetCurrentSessionForSubstrate {
  /// Get the session.
  fn get() -> u32;
}

#[frame_support::pallet]
pub mod pallet {
  use crate::GetCurrentSessionForSubstrate;

  #[pallet::config]
  pub trait Config: frame_system::Config {
    /// The item which tracks the session.
    type Session: GetCurrentSessionForSubstrate;
  }

  #[pallet::pallet]
  pub struct Pallet<T>(_);

  impl<T: Config> Pallet<T> {
    /// The current session index for Substrate's consensus.
    pub fn current_index() -> u32 {
      T::Session::get()
    }
  }
}
pub use pallet::*;
