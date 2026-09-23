//! Adapter crate: byte transports carrying a session's I/O, behind acter-core's
//! `Transport` port.
#![warn(unreachable_pub)]

mod fake;
mod local;
mod scripted;
mod ssh;

pub use fake::{Chunking, FakeShell, Script, Submission, TranscriptShell, Unmarked};
pub use local::LocalPty;
pub use scripted::{ScriptedTransport, SessionTranscript};
pub use ssh::{FarEnd, KnownHosts, SshTarget, SshTransport};

/// How long a far end has to say what it is before the session opens without it.
pub fn probe_patience() -> std::time::Duration {
    ssh::probe::PATIENCE
}
