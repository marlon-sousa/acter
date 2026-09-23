//! Entity/value: what Acter runs inside a session once that session is established, and
//! whether it is allowed to.
//!
//! Sent after the user's startup files have run, so they cannot override it.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::ShellMarkers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSetup {
    /// Shown to the user before it runs.
    pub line: String,
    /// What the far end can mark once this line has run.
    pub markers: ShellMarkers,
}

/// Whether this connection may set its session up; not asking again about a shell is
/// kept separately, behind [`Explained`](crate::Explained).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SetUp {
    /// Ask, unless told not to about this shell, then send the line.
    Yes,
    No,
}

impl SetUp {
    pub fn wanted(self) -> bool {
        self == Self::Yes
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_checkbox_arrives_from_the_wire_as_what_was_ticked() {
        assert_eq!(
            serde_json::from_value::<SetUp>(json!("Yes")).expect("a ticked box arrives"),
            SetUp::Yes
        );
        assert_eq!(
            serde_json::from_value::<SetUp>(json!("No")).expect("and an unticked one"),
            SetUp::No
        );
    }

    #[test]
    fn only_a_ticked_box_wants_anything_run() {
        assert!(SetUp::Yes.wanted());
        assert!(!SetUp::No.wanted());
    }
}
