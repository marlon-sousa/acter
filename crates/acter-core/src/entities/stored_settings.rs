//! Entity/value: the settings document — everything Acter decides on a person's behalf,
//! in one typed structure.
//!
//! A typed structure rather than a map: the compiler finds every reader of a setting
//! whose shape changes, and the frontend stays on named actions rather than a
//! key-and-value surface across the IPC boundary.
//!
//! One document rather than one file per connection, under one lock with one write path,
//! protected by an atomic write that keeps the replaced document and moves aside anything
//! that will not parse.

use serde::{Deserialize, Serialize};

use crate::{AcceptedHostKey, SavedConnection};

/// A document that says a *higher* number was written by a later Acter; still read,
/// because every field is defaulted, but a later format must not change the meaning of a
/// field this one already reads.
pub const FORMAT: u32 = 1;

/// One JSON document, in the settings folder, holding everything Acter writes about a
/// person's choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSettings {
    /// Written from the first document and never absent after.
    #[serde(default = "format_of_the_first_document")]
    pub format: u32,
    /// The saved connections, in the order they were written. Order is not meaning: a
    /// listener meets them alphabetical, decided where the list is built.
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
    /// The server identities somebody accepted. Distinct from the user's own
    /// `~/.ssh/known_hosts`, which Acter reads and never writes.
    #[serde(default)]
    pub host_keys: Vec<AcceptedHostKey>,
    /// Whether a new connection coming up should offer to save itself. True until the
    /// user ticks "Do not offer to save new connections", the only thing that writes it.
    #[serde(default = "offer_by_default")]
    pub offer_to_save: bool,
}

/// What a document with no `format` is read as: it can only be one somebody wrote by
/// hand, since Acter has written the field since the first document it ever wrote.
fn format_of_the_first_document() -> u32 {
    FORMAT
}

/// A person who has never met the offer cannot have decided against it.
fn offer_by_default() -> bool {
    true
}

impl Default for StoredSettings {
    /// Also what a folder with no document at all means: an ordinary first run, not an
    /// error.
    fn default() -> Self {
        Self {
            format: FORMAT,
            connections: Vec::new(),
            host_keys: Vec::new(),
            offer_to_save: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{SavedTarget, SetUp};

    #[test]
    fn an_empty_document_is_the_format_and_nothing_saved() {
        let written = serde_json::to_value(StoredSettings::default()).expect("it serializes");

        assert_eq!(
            written,
            json!({
                "format": 1,
                "connections": [],
                "host_keys": [],
                "offer_to_save": true
            })
        );
    }

    #[test]
    fn a_document_missing_everything_but_its_braces_still_loads() {
        let read: StoredSettings = serde_json::from_value(json!({})).expect("it still loads");

        assert_eq!(read, StoredSettings::default());
    }

    #[test]
    fn a_document_from_a_later_acter_is_read_rather_than_refused() {
        let later = json!({
            "format": 99,
            "connections": [],
            "offer_to_save": false,
            "something_nobody_here_has_heard_of": 3
        });

        let read: StoredSettings = serde_json::from_value(later).expect("it loads");

        assert_eq!(read.format, 99);
        assert!(!read.offer_to_save);
    }

    #[test]
    fn a_saved_connection_comes_back_out_of_the_document_unchanged() {
        let document = StoredSettings {
            connections: vec![SavedConnection {
                name: "work laptop".to_owned(),
                target: SavedTarget::Ssh {
                    host: "example.org".to_owned(),
                    port: 2222,
                    account: "marlon".to_owned(),
                },
                set_up: SetUp::No,
                line_owner: crate::LineOwner::Local,
            }],
            ..StoredSettings::default()
        };

        let text = serde_json::to_string(&document).expect("it writes");
        let back: StoredSettings = serde_json::from_str(&text).expect("and reads");

        assert_eq!(back, document);
        assert!(
            !text.contains("password") && !text.contains("secret"),
            "there is no field a password could be in: {text}"
        );
    }
}
