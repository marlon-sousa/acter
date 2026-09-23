//! Entity/value: the autoread verdict — how much of a span should be spoken.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadMode {
    Auto,
    /// Announced by size, not read aloud.
    TooBig,
    /// Goes to the buffer unannounced.
    Quiet,
}
