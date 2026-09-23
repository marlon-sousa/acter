//! Entity/value: the settings document — everything Acter decides on a person's behalf.

use serde::{Deserialize, Serialize};

use crate::{AcceptedHostKey, SavedConnection};

/// A later format may add fields but must not change the meaning of one this format reads.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSettings {
    #[serde(default = "format_of_the_first_document")]
    pub format: u32,
    /// In the order written; the list is sorted where it is built.
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
    /// Acter never writes `~/.ssh/known_hosts`; its accepted keys live here.
    #[serde(default)]
    pub host_keys: Vec<AcceptedHostKey>,
    #[serde(default = "offer_by_default")]
    pub offer_to_save: bool,
}

fn format_of_the_first_document() -> u32 {
    FORMAT
}

fn offer_by_default() -> bool {
    true
}

impl Default for StoredSettings {
    /// Also what a missing document means.
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
