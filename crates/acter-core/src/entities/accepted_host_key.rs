//! Entity/value: one server identity Acter has accepted — which machine, which kind of
//! key, the fingerprint, and the day somebody said yes.

use serde::{Deserialize, Serialize};

/// Only `Acter` records are ever written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostKeyOrigin {
    #[default]
    Acter,
    /// Read from `~/.ssh/known_hosts`.
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedHostKey {
    /// As typed into the Connect dialog.
    pub host: String,
    /// Kept even when it is 22: a host on two ports is two identities.
    pub port: u16,
    /// As OpenSSH names it; a key under another algorithm is unknown, not changed.
    pub algorithm: String,
    /// As `ssh-keygen -l` prints it.
    pub fingerprint: String,
    /// `2026-09-12`, and empty for a native record.
    pub accepted: String,
    /// Not serialized: every record in the document is Acter's own.
    #[serde(skip)]
    pub origin: HostKeyOrigin,
}

impl AcceptedHostKey {
    /// Exact comparison, correct only for Acter's own records; `known_hosts` has wildcards
    /// and hashed hosts.
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
