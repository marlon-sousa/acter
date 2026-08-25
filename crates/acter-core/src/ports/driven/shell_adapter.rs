//! Port (driven): what the domain needs to know about one shell — how to start it, how far
//! its own command-boundary markers reach, and what ends it.
//!
//! **Facts consumed by different things.** The launch is what the transport spawns; the
//! markers are what the boundary tracker believes; the end-of-input answer is what the
//! session writes when a keystroke means "there is no more input". B4.5 measured why the
//! first two must not be chosen apart: injecting cmd's prompt markers without telling the
//! domain that the shell emits no `C` produces a session that receives markers, opens no
//! block and speaks nothing at all. Before this port there was a branch in the composition
//! root computing both side by side and trusting itself to keep them in step; now one
//! object owns them, so a shell that is wrong is wrong in one place.
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

/// What the *session* needs to know about the shell it is talking to: how far its markers
/// reach, and what ends its input.
///
/// **Two facts in one value, rather than two more parameters.** `SessionService::start`
/// took `ShellMarkers` alone until B5.2 gave the domain a second thing to ask a shell, and
/// a service that grows an argument per shell fact is a service whose call sites have to be
/// edited by every entry in B5. Bundling them is the same reasoning [`ShellLaunch`] is
/// built on, applied to the other side of the seam: the launch is what the transport needs,
/// this is what the session needs, and each travels as one thing.
///
/// **The service is handed values rather than the adapter itself**, deliberately. It reads
/// both facts once at construction and holds them for the life of the session, so a
/// borrowed `&dyn ShellAdapter` would be a reference the service does not keep and cannot
/// use again — and every test that starts a session would need an adapter to hand it, even
/// the ones whose whole subject is a shell with the injection deliberately taken away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellFacts {
    /// How far this shell's own markers reach.
    pub markers: ShellMarkers,
    /// The bytes that end its input, or `None` for a shell nobody has measured.
    pub eof: Option<Vec<u8>>,
}

impl ShellFacts {
    /// What this adapter says about itself, which is how the composition root builds one.
    pub fn of(shell: &dyn ShellAdapter) -> Self {
        Self {
            markers: shell.markers(),
            eof: shell.eof(),
        }
    }
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

    /// What to write when the user says there is no more input, or `None` for a shell
    /// whose answer nobody has measured.
    ///
    /// **A method on this port because the answer differs per shell over one transport**,
    /// which is DESIGN's transport-versus-shell criterion read the other way round from
    /// [`Transport::interrupt`](crate::Transport::interrupt): interrupting is one shell
    /// over two transports and therefore belongs to the transport, and ending input is one
    /// transport under two shells and therefore belongs here. B6 filed it against
    /// whichever entry first had a shell that needed it.
    ///
    /// **Bytes rather than a keystroke, and the reason is what B5.2 measured.** The
    /// obvious shape is "which control byte", and PowerShell has none: neither `0x1a` nor
    /// `0x04` ends a session on a pseudoconsole, both are echoed as caret text, and a line
    /// submitted behind one runs as a command the user never typed. What does end it is
    /// the line `exit`, so an answer that could only be a control byte would have had to
    /// say "nothing" for the shell this method was introduced for.
    ///
    /// `None` is the honest answer for a shell nobody has measured, and it is not the same
    /// as "this shell cannot be ended": it is "Acter does not know how", which the session
    /// reports rather than guessing at a byte and leaving text on the line.
    fn eof(&self) -> Option<Vec<u8>>;
}
