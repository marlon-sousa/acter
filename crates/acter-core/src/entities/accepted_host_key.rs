//! Entity/value: one server identity Acter has accepted — which machine, which kind of
//! key, the fingerprint, and the day somebody said yes.
//!
//! **It lives in the settings document, under a key of its own** (spec 26, decision 2, as
//! amended in implementation on 2026-09-12 at the user's asking). The first version of that
//! decision left `known_hosts` beside the document as a file in OpenSSH's own format,
//! unchanged. What it is now is a typed list in the one document, which is what the rest of
//! that decision already argued for: one document under one lock with one write path, and
//! nothing in it a free-text key. Acter's record of host keys is something Acter decided on
//! a person's behalf, and that sentence is what says where it belongs.
//!
//! **The user's own `~/.ssh/known_hosts` is untouched by this and stays a file.** Acter
//! reads it and never writes it (spec B9, decision 5), and it is not Acter's to move: a
//! record kept for `ssh` belongs where `ssh` keeps it, in the format `ssh` reads.
//!
//! # Why the fingerprint rather than the key
//!
//! **It is what a person can actually check.** A base64 key is sixty-eight characters of
//! noise; the fingerprint is what a hosting provider prints, what a colleague reads out,
//! and what the dialog put in front of the user when they accepted it — so a record they
//! can open and compare is a record in that form. Comparing a server's offer against it is
//! the same operation either way: the fingerprint of what is offered is computed and
//! matched, exactly as the dialog computed it.
//!
//! **The algorithm is kept beside it** because two facts need it and neither is
//! cosmetic: a key recorded under a *different* algorithm is an unknown key rather than a
//! changed one, and the algorithms already on file are what Acter offers the server first
//! so a familiar host is not asked about on every second connection.
//!
//! **And the day, because a record nobody can date is a record nobody can audit.** "When
//! did I trust this?" is the question somebody asks when a key changes, and it is the one
//! question the old file could not answer.
//!
//! # One list, and half of it is not Acter's to keep
//!
//! **The user's own `~/.ssh/known_hosts` is read into the same list**, so anything asking
//! what is known about a server gets one answer rather than two it has to merge (asked for
//! by the user, 2026-09-12). What tells the halves apart is [`HostKeyOrigin`], and it is
//! there for exactly one reason: a record read out of somebody else's file must never be
//! written into Acter's document. Acter reads that file and never writes it (spec B9,
//! decision 5), and a copy of it inside the settings document would be Acter writing it
//! by another route — stale the moment `ssh` changed it, and impossible for the user to
//! correct from the place they would go to correct it.
//!
//! **The flag is not in the document, structurally.** It is `#[serde(skip)]`, so it cannot
//! be written and cannot be read back wrong; a record loaded from the document is Acter's
//! own by construction, which is the only thing a document can hold.

use serde::{Deserialize, Serialize};

/// Where a record came from, and therefore whether Acter may write it down.
///
/// **Not in the document** — it is `#[serde(skip)]` on the field below, because a record
/// that is in the document is Acter's own by construction and a record that is not can
/// never get there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostKeyOrigin {
    /// Acter wrote it down, because somebody accepted this server in Acter's own dialog.
    /// The default, because it is the only thing the document can hold.
    #[default]
    Acter,
    /// Read out of the user's own `~/.ssh/known_hosts`. **Never written anywhere**: it is
    /// not Acter's record, and a copy of it in Acter's document would be stale the moment
    /// `ssh` changed it.
    Native,
}

/// One server whose identity this person accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedHostKey {
    /// The machine, as it was typed into the Connect dialog.
    pub host: String,
    /// The port it was reached on. **Kept even when it is 22**, because a host on two ports
    /// is two identities and nothing about the record should have to guess which.
    pub port: u16,
    /// Which kind of key, as OpenSSH names it: `ssh-ed25519`, `ssh-rsa`,
    /// `ecdsa-sha2-nistp256`.
    pub algorithm: String,
    /// The key's fingerprint as `ssh-keygen -l` prints it: `SHA256:` and unpadded base64 —
    /// the same string the dialog showed when this was accepted.
    pub fingerprint: String,
    /// The day it was accepted, as `2026-09-12`.
    ///
    /// **A day and not a moment.** What it answers is "when did I trust this", and an hour
    /// and a minute add nothing to that while making the line longer to read.
    ///
    /// Empty for a [`HostKeyOrigin::Native`] record: `ssh` does not date its own file, and
    /// inventing a day for something somebody else wrote would be inventing a fact.
    pub accepted: String,
    /// Whether Acter wrote this down or read it out of the user's own file.
    ///
    /// **Skipped on the way to and from the document.** A record in the document is
    /// Acter's own; a native one is assembled at the moment it is read and lives no longer
    /// than the question it answers.
    #[serde(skip)]
    pub origin: HostKeyOrigin,
}

impl AcceptedHostKey {
    /// Whether this record is about that server.
    ///
    /// **An exact comparison, and that is safe here where it would not be for the user's
    /// file.** Acter writes these itself and writes the host as it was typed, so there are
    /// no wildcards to expand and no hashed host lines to verify. `~/.ssh/known_hosts` can
    /// hold both, which is why it is still read by the SSH library rather than by this.
    pub fn is_for(&self, host: &str, port: u16) -> bool {
        self.host == host && self.port == port
    }

    /// Whether this is Acter's to keep. **The one question the origin exists to answer.**
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

    /// The document a person opens: five named fields, and the fingerprint spelled the way
    /// `ssh-keygen -l` spells it — because comparing it against what a provider printed is
    /// the only thing anybody does with it.
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

    /// A host on two ports is two identities, and nothing here treats them as one.
    #[test]
    fn a_record_is_about_one_host_on_one_port() {
        assert!(accepted(2222).is_for("example.org", 2222));
        assert!(!accepted(2222).is_for("example.org", 22));
        assert!(!accepted(2222).is_for("example.com", 2222));
    }

    /// There is no field a private key could be in, and nothing here is a secret: a host
    /// key is public by construction, and its fingerprint is what gets read out loud.
    #[test]
    fn nothing_in_a_record_is_a_secret() {
        let written = serde_json::to_string(&accepted(22)).expect("it writes");

        for forbidden in ["password", "secret", "private"] {
            assert!(!written.contains(forbidden), "{written}");
        }
    }

    /// **A record's origin never reaches the document**, which is the whole of the
    /// guarantee: a native record cannot be written, and one that is in the document is
    /// Acter's own whatever it says.
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

    /// And the two are told apart in memory, which is what stops the one Acter may not
    /// keep from being kept.
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
