//! Entity/value: a recognized OSC 133 shell-integration marker.

use crate::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Osc133Marker {
    /// OSC 133 A.
    PromptStart,
    /// OSC 133 B.
    CommandStart,
    /// OSC 133 C.
    OutputStart,
    /// OSC 133 D. `None` for a bare `D` or an unparseable code.
    CommandEnd(Option<ExitCode>),
}
