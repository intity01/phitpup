//! OS-independent core of PhitPup.
//!
//! Text typed with the wrong keyboard layout is fixed by key position: each
//! character is traced back to the physical key (and Shift state) that produced
//! it in one layout, then replaced by what that key produces in the other.
//! [`Detector`] decides, for suggest and auto mode, whether a word needs that.
//! Nothing here calls the operating system, so it builds for WASM and tests on
//! any platform.

pub mod config;
pub mod convert;
pub mod detect;
pub mod fix;
pub mod hotkey;
pub mod icon;
pub mod layouts;
pub mod model;
pub mod near;
pub mod thai;
pub mod typo;
pub mod update;

pub use convert::Converter;
pub use detect::{Detector, Direction, Thresholds, Verdict};
pub use fix::{Edit, Fixer, Kind, Suggestion};
pub use layouts::{Key, Layout};
