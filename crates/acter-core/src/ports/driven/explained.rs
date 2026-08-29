//! Port (driven): which shells this person has already had Acter's setup command explained
//! to them for, and has said not to be asked about again.
//!
//! **It is kept per shell, and that is the user's decision rather than a convenience** (spec
//! B9.5, decision 10): *"we would want to be sure the user has a chance to review commands
//! for all the shells they use."* Somebody who has read and accepted what Acter runs in bash
//! has not thereby accepted what it runs in zsh, and a shell whose setup is added by a later
//! entry asks again — as it should, because it is a different command.
//!
//! **It is not keyed by host and not keyed by profile.** Tying it to a connection would
//! re-explain the same command for every new host, which is the thing that teaches people to
//! dismiss dialogs without reading them.
//!
//! **A port of its own rather than a corner of the profile store**, because the two are
//! different things kept for different reasons (decision 10). The Connect dialog's checkbox
//! is a property of a *connection* and belongs to the saved profile, which is B8's; this is a
//! property of the *person*, and it outlives every profile they ever make.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. Where the answer is kept
//! is the adapter's business and the composition root's — a file under the directory
//! `acter_known_hosts` already resolves, which is the precedent B9 set for Acter's own record
//! of host keys before B8 existed.

/// What this person has already been shown and does not want shown again.
///
/// `Send + Sync` for [`Signatures`](crate::Signatures)' reason: the composition root hands one
/// to a factory that runs on whichever thread answered an invoke.
pub trait Explained: Send + Sync {
    /// Whether this shell's setup command has already been explained to this person and
    /// dismissed.
    ///
    /// **The safe answer is `false`**, which asks again. A store that cannot be read has not
    /// discovered that somebody consented; the cost of asking twice is a dialog, and the cost
    /// of not asking is a command run in somebody's session that they were never shown.
    fn already(&self, shell: &str) -> bool;

    /// Remember that it has been, so this shell does not ask again.
    ///
    /// Answers nothing: a preference that could not be written down is not worth interrupting
    /// a connection over, and the consequence is that the dialog appears once more.
    fn remember(&self, shell: &str);
}

/// Nowhere to keep it, so nothing was ever explained.
///
/// The honest null implementation: every shell asks, every time. It is what a build with no
/// place to write to uses, and what a test that is not about the preference uses.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeverExplained;

impl Explained for NeverExplained {
    fn already(&self, _shell: &str) -> bool {
        false
    }

    fn remember(&self, _shell: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With nowhere to keep the answer, the question is asked — which is the direction that
    /// costs a dialog rather than the one that costs disclosure.
    #[test]
    fn with_nowhere_to_keep_it_every_shell_is_asked_about_again() {
        let explained = NeverExplained;
        explained.remember("bash");

        assert!(!explained.already("bash"));
    }
}
