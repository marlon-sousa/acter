//! Port (driven): the byte transport carrying one session's I/O — the seam between the
//! domain and whatever is actually on the other end of the wire: a shell on a local
//! PTY, an SSH channel, or a scripted transcript standing in for either.
//!
//! **Reads are pushed, and the adapter owns its reading strategy.** [`Transport::start`]
//! is handed the sending half of a channel and is free to fill it however it likes:
//! ARCHITECTURE gives a blocking PTY read a dedicated thread feeding the session
//! channel, while a scripted transport runs a [`Clock`](crate::Clock)-driven loop. That
//! is exactly the knowledge that differs between implementers and that nothing above
//! them can express, so a blocking `read(&mut self, buf) -> io::Result<usize>` was
//! rejected: it is a smaller trait, but it moves thread ownership up into the domain
//! and forces a future async transport to pretend to block (spec B3.5, decision 1).
//!
//! **The end of a session is the channel closing, not an error variant.** A shell that
//! exits is not a failure, and the domain already has to handle its input channel
//! ending. Only the write side reports errors, because a write is a thing the user just
//! asked for and a failure has to be sayable out loud.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits — no `async_trait`
//! machinery — and fire-and-forget on the read side, the shape
//! [`EventSink`](crate::EventSink) already has in the other direction.

use thiserror::Error;
use tokio::sync::mpsc::Sender;

/// One session's byte transport.
///
/// `Send` because the session actor owns its transport on a task of its own; not
/// `Sync`, because every method takes `&mut self` and nothing shares one.
pub trait Transport: Send {
    /// Starts the session and begins delivering reads to `bytes`.
    ///
    /// One send is one read: an implementer must never merge two reads into a single
    /// delivery to make its stream tidier, because chunk boundaries are exactly what
    /// DESIGN's reliability cases are about — a marker split across two reads is the
    /// case the domain has to survive, and a transport that hides it hides the bug.
    ///
    /// Returns nothing. Once started, the only way this ends is the channel closing:
    /// dropped by the domain when the session is torn down, or dropped by the adapter
    /// when the far end went away.
    fn start(&mut self, bytes: Sender<Vec<u8>>);

    /// Writes bytes toward the far end: a submitted command line, a control byte, or a
    /// device-query answer the terminal engine produced.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError>;

    /// Tells the far end the screen changed size. Separate from
    /// [`TerminalEngine::resize`](crate::TerminalEngine::resize), which resizes the
    /// emulated grid: both have to happen, and only one of them is I/O.
    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError>;
}

/// Why a write or a resize could not be delivered.
///
/// The `Display` strings are whole spoken sentences rather than the lowercase fragments
/// `thiserror` conventionally carries: every one of these reaches a screen reader user
/// as the answer to "what happened to what I just typed", and CLAUDE.md makes that a
/// domain requirement rather than polish.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    /// Written to before the session was started. A caller ordering bug rather than a
    /// world failure, but it is still said out loud rather than swallowed.
    #[error("The session has not started yet, so there is nothing to send to.")]
    NotStarted,

    /// The far end is gone: the shell exited, the connection dropped, the scripted
    /// session ended. Nothing was written and nothing will be.
    #[error("The session has ended, so the text could not be sent.")]
    Closed,

    /// The transport is alive but this write failed. `detail` comes from the world (an
    /// operating-system error, usually), so it is appended as its own sentence rather
    /// than spliced into one — a reader reaches it after the plain-language part.
    #[error("The text could not be sent to the session. {detail}")]
    Failed { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, so a new one cannot be added without deciding what it says.
    fn every_variant() -> Vec<TransportError> {
        vec![
            TransportError::NotStarted,
            TransportError::Closed,
            TransportError::Failed {
                detail: "The pipe was closed by the far end.".to_owned(),
            },
        ]
    }

    /// The one behavior this file has, and the one worth pinning: these strings are
    /// spoken. A fragment like "not started" would reach a listener mid-sentence with
    /// no subject, which is the failure this test exists to prevent.
    #[test]
    fn every_error_speaks_a_whole_sentence() {
        for error in every_variant() {
            let spoken = error.to_string();
            let first = spoken.chars().next().expect("a message is never empty");
            assert!(
                first.is_uppercase(),
                "a spoken message starts a sentence: {spoken}"
            );
            assert!(
                spoken.ends_with('.'),
                "a spoken message ends in a full stop, so a reader pauses: {spoken}"
            );
            assert!(
                spoken.split_whitespace().count() >= 5,
                "a spoken message says what happened, not a label: {spoken}"
            );
        }
    }

    #[test]
    fn a_failure_carries_the_world_s_own_words_after_the_plain_language_part() {
        let error = TransportError::Failed {
            detail: "The handle is invalid.".to_owned(),
        };
        let spoken = error.to_string();
        assert!(spoken.starts_with("The text could not be sent"));
        assert!(spoken.ends_with("The handle is invalid."));
    }
}
