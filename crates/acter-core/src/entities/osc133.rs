//! Entity/value: a recognized OSC 133 shell-integration marker.
//!
//! Recognition itself is not here and not in this crate: the terminal engine wrapper in
//! `acter-term` already runs a real escape-sequence parser over the byte stream, and a
//! second parser could disagree with the first about what is a real sequence (spec B2,
//! decision 1). This type is what that parser dispatches, and what
//! [`BoundaryTracker`](crate::BoundaryTracker) consumes.

use crate::ExitCode;

/// The four markers DESIGN's command-boundary section defines. The prompt only
/// reappears once the previous command has ended, which is what makes
/// [`CommandEnd`](Osc133Marker::CommandEnd) a deterministic end signal rather than a
/// heuristic — and what makes an earlier marker mid-block a sign that something lied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Osc133Marker {
    /// **A** — the shell began drawing its prompt.
    PromptStart,
    /// **B** — the command line was accepted; what follows is the shell's echo of it.
    CommandStart,
    /// **C** — the command is running; what follows is its output.
    OutputStart,
    /// **D** — the command finished.
    ///
    /// The exit code is optional because a real marker can arrive without one: shells
    /// emit a bare `D` in some paths, and a forged marker may carry nothing parseable.
    /// The alternative — a sentinel inside [`ExitCode`] — would leak this into the wire
    /// format, since `ExitCode` is a protocol type (spec B2, decision 6).
    CommandEnd(Option<ExitCode>),
}
