//! Adapter: a far end that is not on this machine — an SSH connection, and what Acter has
//! to know and ask before there is a session on it.

mod known_hosts;
pub(crate) mod probe;
mod transport;

pub use known_hosts::KnownHosts;
pub use probe::FarEnd;
pub use transport::{SshTarget, SshTransport};
