//! Adapter crate: wraps the terminal emulation engine behind acter-core's
//! `TerminalEngine` port — bytes in; identified lines of extracted text, recognized
//! OSC 133 markers, and alt-screen transitions out.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod alacritty_engine;

pub use alacritty_engine::AlacrittyEngine;
