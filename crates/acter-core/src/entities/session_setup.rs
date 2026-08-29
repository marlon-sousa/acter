//! Entity/value: what Acter runs inside a session once that session is established, and
//! whether it is allowed to.
//!
//! **The session is set up after it is established, not before it starts** (spec B9.5,
//! decision 1). Everything Acter used to arm at launch went into the environment the shell
//! was started with, which meant Acter went first and every startup file the user owns got
//! the last word — the ordering behind all four of 23.11's failures, one of which made Acter
//! announce that a failed command had succeeded. A line sent into the session once it is up
//! is the only ordering in which Acter has the last word.
//!
//! **The line and the marker claim travel together**, for the reason
//! [`ShellLaunch`](crate::ShellLaunch) and [`ShellFacts`](crate::ShellFacts) are each one
//! value: what a setup earns is a property of that setup and not of the shell's name. bash's
//! program reaches all four boundaries; POSIX `sh` has `PS1` and no prompt hook, so its own
//! program reaches the prompt boundaries and no further (spec B9.5, decision 8) — and a
//! session told the wrong one of those waits for boundaries that are never coming, or is
//! flagged for the absence of markers it was never going to get.
//!
//! Which line belongs to which shell is a policy and lives in `acter-shells`; this is the
//! shape of its answer.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::ShellMarkers;

/// One shell's setup: the line to run inside the session, and how far the markers that line
/// earns reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSetup {
    /// The command, verbatim.
    ///
    /// **It is shown to the user before it runs and it is not hidden afterwards** (spec
    /// B9.5, decision 3): the dialog puts it in a field that can be read character by
    /// character, it is submitted through the same path a typed line takes, it heads its own
    /// block, and the shell's own history keeps it. There is nothing here Acter would rather
    /// a user did not see.
    pub line: String,
    /// What the far end will be able to mark once this line has run.
    pub markers: ShellMarkers,
}

/// Whether this connection may set its session up at all.
///
/// **The checkbox authorises and the dialog discloses, and neither is optional** (spec B9.5,
/// decision 9). This is the checkbox: it is ticked by default, because that is what makes an
/// ordinary user hear headings and failures without knowing the words this project uses, and
/// unticking it has to be reachable without the dialog ever appearing.
///
/// **It travels with the attempt rather than being stored**, until B8 has a profile to keep
/// it in (decision 10). Which shells this person has said not to be asked about again is a
/// different thing entirely and is kept behind [`Explained`](crate::Explained).
///
/// A two-variant enum rather than a `bool`, for [`ProgramAnswer`](crate::ProgramAnswer)'s
/// reason: `true` at a call site three files away does not say which way it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SetUp {
    /// Set it up: ask about it unless this person has said not to, then send the line.
    Yes,
    /// Do not. No dialog, no setup line, and the connection says what the session will and
    /// will not be able to tell them.
    No,
}

impl SetUp {
    /// Whether anything is to be run at all.
    pub fn wanted(self) -> bool {
        self == Self::Yes
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The checkbox crosses the invoke boundary, so it has to survive the round trip the
    /// frontend puts it through.
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
