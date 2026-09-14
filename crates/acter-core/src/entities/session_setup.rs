//! Entity/value: what Acter runs inside a session once that session is established, and
//! whether it is allowed to.
//!
//! Sent after the session is established, not before it starts: a startup file the user
//! owns is loaded first and would otherwise get the last word over Acter's own line.
//!
//! What a setup earns is a property of the setup, not of the shell's name: bash's program
//! reaches all four boundaries, while POSIX `sh` has `PS1` and no prompt hook and so
//! reaches only the prompt boundary. Which line belongs to which shell is a policy and
//! lives in `acter-shells`; this is the shape of its answer.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::ShellMarkers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSetup {
    /// Shown to the user before it runs, and not hidden afterwards.
    pub line: String,
    /// What the far end will be able to mark once this line has run.
    pub markers: ShellMarkers,
}

/// Whether this connection may set its session up at all. Ticked by default. Travels
/// with the attempt; not persisted. Distinct from whether this person has said not to be
/// asked about a given shell again, which is kept behind [`Explained`](crate::Explained).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SetUp {
    /// Ask about it unless this person has said not to, then send the line.
    Yes,
    /// No dialog, no setup line.
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
