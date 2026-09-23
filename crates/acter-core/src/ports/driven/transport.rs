//! Port (driven): the byte transport carrying one session's I/O.

use thiserror::Error;
use tokio::sync::mpsc::Sender;

pub trait Transport: Send {
    /// One send is one read, never two merged; the session ends when this channel closes,
    /// not with an error.
    fn start(&mut self, bytes: Sender<Vec<u8>>);

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError>;

    fn interrupt(&mut self) -> Result<(), TransportError>;

    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError>;
}

/// Every `Display` string is a whole spoken sentence.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    #[error("The session has not started yet, so there is nothing to send to.")]
    NotStarted,

    #[error("The session has ended, so the text could not be sent.")]
    Closed,

    #[error("The text could not be sent to the session. {detail}")]
    Failed { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_variant() -> Vec<TransportError> {
        vec![
            TransportError::NotStarted,
            TransportError::Closed,
            TransportError::Failed {
                detail: "The pipe was closed by the far end.".to_owned(),
            },
        ]
    }

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
