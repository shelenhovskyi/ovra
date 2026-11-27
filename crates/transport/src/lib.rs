//! Transport between a front end and the core.
//!
//! Every transport (Tauri IPC, WebSocket) carries the same JSON frames, so the
//! UI and the core never know which one is in use. See `docs/architecture.md`.
//!
//! * client → core: `{"id": 1, "method": "get_info", "params": ...}`
//! * core → client: `{"id": 1, "result": ...}`, `{"id": 1, "error": ...}`
//!   or `{"event": ...}`

mod frame;
#[cfg(feature = "stdio")]
pub mod stdio;
#[cfg(feature = "ws")]
pub mod ws;

pub use frame::{ServerFrame, dispatch, event_frame};
