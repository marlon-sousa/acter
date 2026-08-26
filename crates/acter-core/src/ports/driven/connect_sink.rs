//! Port (driven): where the steps of a connection go while it is being made.
//!
//! The other direction from [`SshQuestions`](crate::SshQuestions), and the pair is what
//! makes connecting a conversation: this carries what is happening and what is being asked
//! *out* to whoever is in front of the window, and the answer comes back through the
//! question port that parked waiting for it.
//!
//! **Fire-and-forget, the shape [`EventSink`](crate::EventSink) already has.** A step is
//! delivered or it is not; there is nothing a caller could do about a frontend that has
//! gone away, and the connection it belongs to will fail on its own terms.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. In production the
//! adapter behind it is a Tauri `Channel` — ordered and bound to the caller who started
//! this attempt, which is what `attach_session` uses for the same reason.

use crate::ConnectStep;

/// Where a connection reports what it is doing.
///
/// `Send + Sync` because the connection runs on a task of its own and reports from there.
pub trait ConnectSink: Send + Sync {
    /// One step, in the order it happened.
    fn send(&self, step: ConnectStep);
}
