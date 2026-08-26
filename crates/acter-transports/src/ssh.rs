//! Adapter: a far end that is not on this machine — an SSH connection, and what Acter has
//! to know and ask before there is a session on it.
//!
//! **The first transport that conducts a conversation.** Every far end before this one
//! could report failure as a single speakable sentence, because starting it was one call to
//! the operating system. Establishing an SSH connection stops on things that are not
//! failures — a host key nobody has seen, a password nobody has typed — and each of them is
//! a question the *user* answers before there is a session to answer it in (spec B9).
//!
//! - [`KnownHosts`] is what is already known about a server's identity, and where a newly
//!   accepted key is written down.
//! - The questions themselves are `acter_core::SshQuestions`, a port, so this module never
//!   opens a dialog and is measured against a real server with no window anywhere near it.
//!
//! Facade: this file only declares modules and re-exports.

mod known_hosts;
pub(crate) mod probe;
mod transport;

pub use known_hosts::KnownHosts;
pub use probe::FarEnd;
pub use transport::{SshTarget, SshTransport};
