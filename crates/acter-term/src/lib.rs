//! Adapter crate: wraps the terminal emulation engine behind acter-core's
//! `TerminalEngine` port — bytes in; grid state, extracted text, and alt-screen
//! transitions out.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]
