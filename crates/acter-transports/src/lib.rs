//! Adapter crate: byte transports carrying a session's I/O, behind acter-core's
//! `Transport` port. A scripted transcript first; local ConPTY and SSH later.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod scripted;

pub use scripted::{ScriptedTransport, SessionTranscript};
