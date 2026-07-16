//! Composition root and Tauri delivery layer: routers (framework adapters),
//! controllers, and the wiring of concrete adapters into core services.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod composition;
mod echo;
mod routers;

pub use composition::run;
