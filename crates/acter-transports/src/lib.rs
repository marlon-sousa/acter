//! Adapter crate: byte transports carrying a session's I/O, behind acter-core's
//! `Transport` port. A scripted session first; local ConPTY and SSH later.
//!
//! The scripted session is two things that compose rather than one that is authored
//! twice: a [`FakeShell`] decides what the far end says, and [`ScriptedTransport`]
//! decides how those bytes arrive (spec B3.6).
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod fake;
mod scripted;

pub use fake::{Chunking, FakeShell, Script, Submission, TranscriptShell, Unmarked};
pub use scripted::{ScriptedTransport, SessionTranscript};
