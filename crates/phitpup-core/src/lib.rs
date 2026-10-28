//! OS-independent core of PhitPup.
//!
//! Text typed with the wrong keyboard layout is fixed by key position: each
//! character is traced back to the physical key (and Shift state) that produced
//! it in one layout, then replaced by what that key produces in the other.
//! Nothing here calls the operating system, so it builds for WASM and tests on
//! any platform.

pub mod convert;
pub mod layouts;

pub use convert::Converter;
pub use layouts::{Key, Layout};
