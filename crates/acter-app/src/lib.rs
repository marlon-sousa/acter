//! Composition root and Tauri delivery layer: routers (framework adapters), the
//! entity/value config, the fake session service, the Channel event-sink adapter, and
//! the container that wires everything. Folders are organized by module role (see
//! ARCHITECTURE.md).
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod adapters;
mod container;
mod entities;
mod routers;
mod services;

pub use container::run;
