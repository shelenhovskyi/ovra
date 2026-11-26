//! Ovra core: the protocol spoken between the UI and the core, and the core itself.
//!
//! This crate must stay free of UI and OS dependencies (see ADR 0002). Every
//! front end (Tauri window, headless daemon, tests) drives the same [`Core`].
//! Work that touches the OS (files, the project database, other apps) is done
//! by services behind the traits in [`services`]; front ends plug in real
//! implementations from other crates, tests plug in the fakes from [`fakes`].

mod agents;
mod audit;
mod browser;
pub mod child_path;
mod companion;
mod context;
mod core;
#[cfg(any(test, feature = "fakes"))]
pub mod fakes;
mod git;
mod lsp;
pub mod netproxy;
mod orchestration;
pub mod path;
mod policy;
mod power;
mod pr;
pub mod protected;
pub mod protocol;
pub mod provenance;
mod remote;
mod resources;
mod search;
mod secrets;
mod servers;
pub mod services;
pub mod untrusted;

pub use crate::core::{Client, Core, CoreBuilder};
pub use crate::orchestration::{Output, PushArgs, Pushed, WorkerState};
