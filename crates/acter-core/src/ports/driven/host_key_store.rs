//! Port (driven): Acter's own record of the server identities somebody accepted (spec 26,
//! decision 2, as amended in implementation on 2026-09-12).
//!
//! **A port rather than a path, because the record moved into the settings document.** It
//! used to be a file in OpenSSH's format, resolved in the composition root and handed to
//! the SSH transport as a `PathBuf`; it is now a typed list in the one document, behind the
//! one object that owns that document and its lock. What the transport is handed is this
//! seam, so the whole of the host-key behaviour stays testable against a record made for
//! the test rather than against whatever this machine happens to hold.
//!
//! **Separate from [`ConnectionStore`](crate::ConnectionStore) although one object
//! implements both.** They are two conversations: one is about the connections a person
//! named, the other about the servers they trusted, and the SSH transport has business
//! with exactly one of them. Decision 10 asks for one *object*, not one port — and a
//! transport that could reach the saved connections would be a transport that could rename
//! one.
//!
//! **The user's own `~/.ssh/known_hosts` is not behind this.** Acter reads it and never
//! writes it (spec B9, decision 5), and it stays a path the SSH adapter reads for itself.

use std::sync::Mutex;

use crate::AcceptedHostKey;

/// What Acter has accepted, and how another one joins it.
///
/// **Only ever Acter's own.** The user's `~/.ssh/known_hosts` joins the same list one
/// layer up, where the SSH library that can parse it lives — and it joins it flagged
/// [`HostKeyOrigin::Native`](crate::HostKeyOrigin), so nothing can hand it back here to be
/// written down. That is structural rather than remembered: [`Self::accept`] takes the
/// facts rather than a record, so there is no native one to pass it.
pub trait HostKeyStore: Send + Sync {
    /// Every server identity accepted so far, all of them Acter's own.
    ///
    /// **Freshly read on every call**, for `connectable`'s reason (spec B7, decision 6): a
    /// host accepted in another window a moment ago is one this window does not ask about.
    fn accepted(&self) -> Vec<AcceptedHostKey>;

    /// Write one down, so the same host is not asked about again.
    ///
    /// **The day is stamped by whoever holds the document**, not by the caller: reading a
    /// clock is the world, and the world is the adapter's.
    ///
    /// The error is a whole spoken sentence, and the caller says what the *consequence* is
    /// rather than only what failed — a key that could not be written down means being
    /// asked again next time, and a user who is told that will not think the question is a
    /// fault.
    fn accept(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String>;
}

/// A record in memory, for the tests and for anything with no document to write to.
///
/// **A fake rather than a mock**, per ARCHITECTURE: it behaves like the real one, so a test
/// asserts what a user would meet rather than which methods were called. The day it stamps
/// is fixed, because a test that asserted today's date would be a test that reads
/// differently tomorrow.
#[derive(Debug, Default)]
pub struct RememberedHostKeys {
    kept: Mutex<Vec<AcceptedHostKey>>,
    /// The sentence every write answers with instead of writing, when a test is about what
    /// a listener hears when a record cannot be kept.
    refusing: Option<String>,
}

/// The day [`RememberedHostKeys`] stamps: the day this record moved into the document.
const A_FIXED_DAY: &str = "2026-09-12";

impl RememberedHostKeys {
    /// A record holding these already.
    pub fn holding(accepted: Vec<AcceptedHostKey>) -> Self {
        Self {
            kept: Mutex::new(accepted),
            refusing: None,
        }
    }

    /// A record that refuses every write with this sentence.
    pub fn refusing(said: &str) -> Self {
        Self {
            kept: Mutex::new(Vec::new()),
            refusing: Some(said.to_owned()),
        }
    }
}

impl HostKeyStore for RememberedHostKeys {
    fn accepted(&self) -> Vec<AcceptedHostKey> {
        self.kept.lock().expect("the fake record's lock").clone()
    }

    fn accept(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String> {
        if let Some(said) = &self.refusing {
            return Err(said.clone());
        }
        self.kept
            .lock()
            .expect("the fake record's lock")
            .push(AcceptedHostKey {
                host: host.to_owned(),
                port,
                algorithm: algorithm.to_owned(),
                fingerprint: fingerprint.to_owned(),
                accepted: A_FIXED_DAY.to_owned(),
                origin: crate::HostKeyOrigin::Acter,
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_that_was_accepted_is_there_the_next_time_it_is_asked_for() {
        let record = RememberedHostKeys::default();

        record
            .accept("example.org", 2222, "ssh-ed25519", "SHA256:abc")
            .expect("it is written down");

        let kept = record.accepted();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].is_for("example.org", 2222));
        assert_eq!(kept[0].fingerprint, "SHA256:abc");
        assert_eq!(kept[0].accepted, A_FIXED_DAY, "the day is stamped for it");
        assert!(kept[0].is_acters_own(), "and it is Acter\'s to keep");
    }

    #[test]
    fn a_record_that_cannot_be_written_answers_a_sentence_and_keeps_nothing() {
        let record = RememberedHostKeys::refusing("The settings folder is not writable.");

        let refused = record.accept("example.org", 22, "ssh-ed25519", "SHA256:abc");

        assert_eq!(
            refused,
            Err("The settings folder is not writable.".to_owned())
        );
        assert!(record.accepted().is_empty());
    }
}
