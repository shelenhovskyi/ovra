//! Ovra core: the protocol spoken between the UI and the core, and the core itself.
//!
//! This crate must stay free of UI and OS dependencies (see ADR 0002). Every
//! front end (Tauri window, headless daemon, tests) drives the same [`Core`].

mod core;
pub mod protocol;

pub use crate::core::Core;
