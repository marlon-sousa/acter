//! Port (driven): Acter's own record of the server identities somebody accepted.

use std::sync::Mutex;

use crate::AcceptedHostKey;

pub trait HostKeyStore: Send + Sync {
    fn accepted(&self) -> Vec<AcceptedHostKey>;

    /// The implementer stamps the day; `Err` is a whole spoken sentence.
    fn accept(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String>;
}

/// A record in memory, for tests, that stamps a fixed day.
#[derive(Debug, Default)]
pub struct RememberedHostKeys {
    kept: Mutex<Vec<AcceptedHostKey>>,
    refusing: Option<String>,
}

const A_FIXED_DAY: &str = "2026-09-12";

impl RememberedHostKeys {
    pub fn holding(accepted: Vec<AcceptedHostKey>) -> Self {
        Self {
            kept: Mutex::new(accepted),
            refusing: None,
        }
    }

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
