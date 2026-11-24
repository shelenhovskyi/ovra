//! Ovra core: the protocol spoken between the UI and the core, and the core itself.
//!
//! This crate must stay free of UI and OS dependencies (see ADR 0002). Every
//! front end (Tauri window, headless daemon, tests) drives the same [`Core`].
//! Work that touches the OS (files, the project database, other apps) is done
//! by services behind the traits in [`services`]; front ends plug in real
//! implementations from other crates, tests plug in the fakes from [`fakes`].

mod core;
#[cfg(any(test, feature = "fakes"))]
pub mod fakes;
pub mod path;
pub mod protocol;
pub mod services;

pub use crate::core::{Core, CoreBuilder};
