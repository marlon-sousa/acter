//! Composition root and Tauri delivery layer: routers (framework adapters), the Channel
//! event-sink and system clock adapters, and the container that wires acter-core's
//! `SessionService` over a byte transport and a terminal engine. Folders are organized by
//! module role (see ARCHITECTURE.md).
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod adapters;
mod container;
mod controllers;
mod routers;

pub use adapters::SystemClock;
pub use container::run;
