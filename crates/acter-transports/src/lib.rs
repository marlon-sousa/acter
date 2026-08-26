//! Adapter crate: byte transports carrying a session's I/O, behind acter-core's
//! `Transport` port. A scripted session first; local ConPTY and SSH later.
//!
//! The scripted session is two things that compose rather than one that is authored
//! twice: a [`FakeShell`] decides what the far end says, and [`ScriptedTransport`]
//! decides how those bytes arrive (spec B3.6). [`LocalPty`] is the other implementer:
//! a real shell on a real pseudoconsole, behind the same port (spec B4).
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod fake;
mod local;
mod scripted;
mod ssh;

pub use fake::{Chunking, FakeShell, Script, Submission, TranscriptShell, Unmarked};
pub use local::LocalPty;
pub use scripted::{ScriptedTransport, SessionTranscript};
pub use ssh::{KnownHosts, SshTransport};
