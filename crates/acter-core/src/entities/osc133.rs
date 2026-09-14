//! Entity/value: a recognized OSC 133 shell-integration marker.
//!
//! Recognition happens once, in `acter-term`'s escape-sequence parser; a second parser
//! here could disagree with it about what is a real sequence.

use crate::ExitCode;

/// The prompt only reappears once the previous command has ended, so an earlier marker
/// mid-block means something lied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Osc133Marker {
    /// OSC 133 A.
    PromptStart,
    /// OSC 133 B.
    CommandStart,
    /// OSC 133 C.
    OutputStart,
    /// OSC 133 D. None when the shell emits a bare `D` or a forged marker carries
    /// nothing parseable.
    CommandEnd(Option<ExitCode>),
}
