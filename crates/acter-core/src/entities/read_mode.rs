//! Entity/value: the autoread verdict — how much of a span should be spoken.
//!
//! Does not cross the frontend wire; the frontend acts on the `Announcement` the
//! verdict produced instead.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadMode {
    Auto,
    /// Announced by size rather than read aloud; a beep signals completion.
    TooBig,
    /// Accumulates silently in the buffer.
    Quiet,
}
