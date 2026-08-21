//! Entity/value: the autoread verdict — how much of a span should be spoken.
//!
//! Computed by [`crate::policies::autoread`] and consumed inside the domain: it
//! decides which [`crate::Announcement`] the actor emits, and
//! [`crate::entities::unspoken_text`] uses it to decide when a span has passed the
//! point of being worth holding. It does **not** cross the frontend wire. It used to,
//! as a field on `Output` and `CommandFinished`, and A6 removed that field: the
//! frontend obeys the announcement the verdict produced, and never sees the verdict
//! itself, so there is exactly one thing for it to act on.

/// The read verdict computed by the pacing policy for one span of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadMode {
    /// Small enough to read aloud automatically.
    Auto,
    /// Over threshold: announced by size rather than read; a beep signals completion.
    TooBig,
    /// Suppressed (e.g. the babble guard tripped); accumulates silently in the buffer.
    Quiet,
}
