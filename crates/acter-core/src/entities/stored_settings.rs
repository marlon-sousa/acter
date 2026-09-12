//! Entity/value: the settings document — everything Acter decides on a person's behalf,
//! in one typed structure (spec 26, decision 2).
//!
//! **Nothing in it is a free-text key.** It is a typed structure rather than a map, which
//! is what lets the compiler find every reader of a setting whose shape changes — and what
//! keeps the frontend on named actions rather than a key-and-value surface across the IPC
//! boundary (decision 11).
//!
//! **[`format`](StoredSettings::format) is there from the first document written**, and
//! every setting this product grows is a new field with a serde default, so a document
//! written by an older Acter still loads and nothing needs migrating for a while.
//!
//! **One document rather than one file per connection**, decided 2026-09-12 against B8's
//! shape. A file each would let somebody copy one, mail one and diff two, and would cost a
//! damaged file only its own connection. That is given up for one document under one lock
//! with one write path, which is what makes a single settings object possible at all: two
//! stores are two things that can disagree about whether a write happened. What pays for
//! the loss is decision 9 — an atomic write, the replaced document kept, and a document
//! that will not parse moved aside rather than written over.

use serde::{Deserialize, Serialize};

use crate::{AcceptedHostKey, SavedConnection};

/// The shape of the document this Acter writes.
///
/// A document that says a *higher* number was written by a later Acter. It is still read,
/// because every field is defaulted and dropping somebody's saved connections over a number
/// would be the worse failure; what a later format may not do is change the meaning of a
/// field this one already reads.
pub const FORMAT: u32 = 1;

/// One JSON document, in the settings folder, holding everything Acter writes about a
/// person's choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSettings {
    /// Which shape this document is, written from the first one and never absent after.
    #[serde(default = "format_of_the_first_document")]
    pub format: u32,
    /// The saved connections, in the order they were written. **Order is not meaning**:
    /// what a listener meets is alphabetical, and that is decided where the list is built
    /// (decision 12).
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
    /// The server identities somebody accepted (decision 2, as amended in implementation
    /// on 2026-09-12).
    ///
    /// **In the document rather than in a file of its own.** It used to be `known_hosts`
    /// beside this, in OpenSSH's format; it is a typed list here, under one lock and one
    /// write path with everything else Acter decided on a person's behalf. The user's own
    /// `~/.ssh/known_hosts` is not this and is not touched: Acter reads it and never writes
    /// it (spec B9, decision 5).
    #[serde(default)]
    pub host_keys: Vec<AcceptedHostKey>,
    /// Whether a new connection coming up should offer to save itself (decision 19).
    ///
    /// **A field here rather than a port of its own**: the settings object is what such a
    /// port would have been, and one document under one lock is the whole of decision 10.
    /// True until somebody ticks "Do not offer to save new connections", which is the only
    /// thing that writes it.
    #[serde(default = "offer_by_default")]
    pub offer_to_save: bool,
}

/// What a document with no `format` is read as. It can only be one somebody wrote by hand,
/// since Acter has written the field since the first document it ever wrote.
fn format_of_the_first_document() -> u32 {
    FORMAT
}

/// Offering is the default, because the offer is how a connection gets saved at all: a
/// person who has never met it cannot have decided against it.
fn offer_by_default() -> bool {
    true
}

impl Default for StoredSettings {
    /// The document a first run has: this format, nothing saved, and the offer on.
    ///
    /// **It is also what a folder with no document at all means** (decision 9). A settings
    /// folder that does not exist yet, or a document that is not there, is an ordinary
    /// first run and not an error.
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

    /// The document a first run writes, asserted as JSON because that is what a person
    /// opening the file meets.
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

    /// **Every field has a default, which is what decision 2 buys**: a document written by
    /// an older Acter still loads, and nothing needs migrating for a while.
    #[test]
    fn a_document_missing_everything_but_its_braces_still_loads() {
        let read: StoredSettings = serde_json::from_value(json!({})).expect("it still loads");

        assert_eq!(read, StoredSettings::default());
    }

    /// And a document written by a *later* Acter is read rather than refused: dropping
    /// somebody's saved connections over a number is the worse failure, and this one has a
    /// default for every field it does not recognise.
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

    /// A saved connection survives the round trip inside the document, which is the whole
    /// of what the file is for.
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
