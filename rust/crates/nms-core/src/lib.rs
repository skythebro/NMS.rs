//! Core of the Rust loader for No Man's Sky mods: a replacement for the parts of pyMHF our mods use.
//!
//! Every behaviour change is a live toggle that stays vanilla when its signature is missing or
//! found more than once. Addresses are never hard-coded: code is found by unique byte patterns.

pub mod image;
pub mod ipc;
pub mod log;
pub mod patch;
pub mod paths;
pub mod pattern;
pub mod registry;
pub mod settings;
pub mod spawn;

#[cfg(windows)]
pub mod hook;
#[cfg(windows)]
pub mod mem;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod shared;

pub use image::{Code, FileImage, Lookup};
pub use patch::{CodePatch, Outcome, PatchResult};
pub use pattern::Pattern;
pub use registry::{GameMod, Kind, Range, Registry, SettingSpec};
pub use settings::Settings;
