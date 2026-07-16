//! Composition root and Tauri delivery layer: routers (framework adapters),
//! services and ports of the temporary echo harness, and the container that wires
//! everything. Folders are organized by module role (see ARCHITECTURE.md).
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod container;
mod ports;
mod routers;
mod services;

pub use container::run;
