//! Entity/value: one server identity Acter has accepted — which machine, which kind of
//! key, the fingerprint, and the day somebody said yes.

use serde::{Deserialize, Serialize};

/// Where a record came from, and therefore whether Acter may write it down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostKeyOrigin {
    /// Somebody accepted this server in Acter's own dialog. The default, because it is
    /// the only thing the document can hold.
    #[default]
    Acter,
    /// Read out of the user's own `~/.ssh/known_hosts`. Never written anywhere: a copy
    /// in Acter's document would be stale the moment `ssh` changed it.
    Native,
}

/// One server whose identity this person accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedHostKey {
    /// The machine, as it was typed into the Connect dialog.
    pub host: String,
    /// Kept even when it is 22: a host on two ports is two identities.
    pub port: u16,
    /// Which kind of key, as OpenSSH names it: `ssh-ed25519`, `ssh-rsa`,
    /// `ecdsa-sha2-nistp256`. A key recorded under a different algorithm reads as an
    /// unknown key, not a changed one.
    pub algorithm: String,
    /// The key's fingerprint as `ssh-keygen -l` prints it: `SHA256:` and unpadded base64
    /// — the same string the dialog showed when this was accepted.
    pub fingerprint: String,
    /// The day it was accepted, as `2026-09-12`. A day and not a moment: an hour and a
    /// minute add nothing to "when did I trust this" while making the line longer to
    /// read.
    ///
    /// Empty for a [`HostKeyOrigin::Native`] record: `ssh` does not date its own file,
    /// and inventing a day for something somebody else wrote would be inventing a fact.
    pub accepted: String,
    /// Whether Acter wrote this down or read it out of the user's own file. Skipped on
    /// the way to and from the document: a record in the document is Acter's own; a
    /// native one is assembled at the moment it is read and lives no longer than the
    /// question it answers.
    #[serde(skip)]
    pub origin: HostKeyOrigin,
}

impl AcceptedHostKey {
    /// Exact comparison, safe only for Acter's own records: they are written with the
    /// host as typed, with no wildcards or hashed host lines to verify, unlike
    /// `~/.ssh/known_hosts`.
    pub fn is_for(&self, host: &str, port: u16) -> bool {
        self.host == host && self.port == port
    }

    pub fn is_acters_own(&self) -> bool {
        self.origin == HostKeyOrigin::Acter
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn accepted(port: u16) -> AcceptedHostKey {
        AcceptedHostKey {
            host: "example.org".to_owned(),
            port,
            algorithm: "ssh-ed25519".to_owned(),
            fingerprint: "SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU".to_owned(),
            accepted: "2026-09-12".to_owned(),
            origin: HostKeyOrigin::Acter,
        }
    }

    #[test]
    fn a_record_is_the_host_the_kind_of_key_the_fingerprint_and_the_day() {
        assert_eq!(
            serde_json::to_value(accepted(2222)).expect("it writes"),
            json!({
                "host": "example.org",
                "port": 2222,
                "algorithm": "ssh-ed25519",
                "fingerprint": "SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU",
                "accepted": "2026-09-12"
            })
        );
    }

    #[test]
    fn a_record_is_about_one_host_on_one_port() {
        assert!(accepted(2222).is_for("example.org", 2222));
        assert!(!accepted(2222).is_for("example.org", 22));
        assert!(!accepted(2222).is_for("example.com", 2222));
    }

    #[test]
    fn nothing_in_a_record_is_a_secret() {
        let written = serde_json::to_string(&accepted(22)).expect("it writes");

        for forbidden in ["password", "secret", "private"] {
            assert!(!written.contains(forbidden), "{written}");
        }
    }

    #[test]
    fn where_a_record_came_from_is_not_something_the_document_holds() {
        let native = AcceptedHostKey {
            origin: HostKeyOrigin::Native,
            ..accepted(22)
        };

        let written = serde_json::to_string(&native).expect("it writes");
        assert!(!written.contains("origin"), "{written}");
        assert!(!written.contains("Native"), "{written}");

        let back: AcceptedHostKey = serde_json::from_str(&written).expect("and reads");
        assert!(
            back.is_acters_own(),
            "a record read out of the document is Acter\'s own by construction"
        );
    }

    #[test]
    fn a_native_record_is_not_acters_to_keep() {
        assert!(accepted(22).is_acters_own());
        assert!(
            !AcceptedHostKey {
                origin: HostKeyOrigin::Native,
                ..accepted(22)
            }
            .is_acters_own()
        );
    }
}
