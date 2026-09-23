//! Facade: the composition root and Tauri delivery layer; declares modules and re-exports the
//! public API.
#![warn(unreachable_pub)]

mod adapters;
mod container;
mod controllers;
mod routers;

pub use adapters::SystemClock;
pub use container::run;
