//! Port (driven): what the domain needs to know about one shell — how to start it, and
//! how far its own command-boundary markers reach.
//!
//! **Two facts, because they are consumed by two different things.** The launch is what
//! the transport spawns; the markers are what the boundary tracker believes. B4.5 measured
//! why they must not be chosen apart: injecting cmd's prompt markers without telling the
//! domain that the shell emits no `C` produces a session that receives markers, opens no
//! block and speaks nothing at all. Before this port there was a branch in the composition
//! root computing both side by side and trusting itself to keep them in step; now one
//! object owns both, so a shell that is wrong is wrong in one place.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits: this is knowledge,
//! not I/O. Discovering *which* shells exist on a machine is I/O and is a different port
//! (spec B5.3).

use crate::ShellMarkers;

/// How one shell is started: the program, the arguments it needs, and the environment its
/// session setup rides in.
///
/// **One value rather than three methods**, because it is one decision. cmd's injection is
/// an environment variable, PowerShell's is a `-Command` argument and WSL's far end is
/// a `-d <distro>` argument, so a caller that could take the environment of one shell and
/// the arguments of another would be assembling a session nobody designed.
///
/// Owned `String`s rather than `&'static str`: cmd's values are constants and could be
/// borrowed, but a distribution name is built at runtime, and a port shaped around the one
/// shell whose data happens to be static would have to be reshaped by the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunch {
    /// The program to spawn, as the user named it: `cmd`, `cmd.exe` or a full path are
    /// the same shell, and which of them reaches the transport is the user's business.
    pub program: String,
    /// The arguments it is started with. Production and every test that measures a shell
    /// take them from here, so the two cannot drift into measuring different streams
    /// (spec B5.1, decision 5).
    pub args: Vec<String>,
    /// Name/value pairs the session is started with. Empty for a shell whose setup is not
    /// an environment variable, and for one Acter knows nothing about.
    pub environment: Vec<(String, String)>,
}

/// One shell Acter can talk to.
///
/// `Send + Sync` because the composition root hands one to a transport built on another
/// task; `&self` throughout because an adapter is knowledge and answers the same thing
/// every time it is asked.
pub trait ShellAdapter: Send + Sync {
    /// How to start this shell.
    fn launch(&self) -> ShellLaunch;

    /// How far this shell's own markers reach — a claim made to the domain, which is why
    /// it is not part of [`ShellLaunch`]: the launch is consumed by the transport and this
    /// is consumed by the session.
    fn markers(&self) -> ShellMarkers;
}
