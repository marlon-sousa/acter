//! Adapter crate: the terminal emulation engine behind acter-core's `TerminalEngine` port.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod alacritty_engine;

pub use alacritty_engine::AlacrittyEngine;
