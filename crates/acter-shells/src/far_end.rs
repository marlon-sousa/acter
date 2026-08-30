//! Policy: what a shell's *name* licenses Acter to believe about it, when Acter did not
//! start that shell and cannot inject anything into it.
//!
//! **A different question from [`adapter_for`](crate::adapter_for), and the difference is
//! the whole of spec B9's decision 7.** That function answers "how do I start this program
//! and what do I inject into it", which is meaningless for a shell on another machine that
//! `sshd` chose from an account's passwd entry and started without asking us. This answers
//! the only two things that still matter about such a shell: what to call it, and what ends
//! its input.
//!
//! **The identity may be guessed from the name; the setup may never be.** Knowing a far end
//! is zsh licenses *saying so* and nothing else until a zsh setup has been measured the way
//! B5.3 measured bash's. A shell with nothing measured for it claims `ShellMarkers::Full` —
//! the same assumption [`Plain`](crate::Plain) makes, which is what lets the grace period flag
//! the session as unintegrated rather than a marker being silently forged.
//!
//! **Since B9.5 an SSH far end can be set up, and it is set up by the same line WSL's is**
//! (spec B9.5, decision 2). Nothing here is a launch argument — Acter controls none over SSH —
//! so the mechanism that reaches a distribution reaches a host on the other side of the world
//! unchanged, which is the first time these two transports have shared a strategy rather than
//! having one each. What a shell's setup earns is [`setup_for`](crate::setup::setup_for)'s
//! answer, and it decides the marker claim: a far end that is being set up claims what its
//! line earns, and one that is not stays optimistic so the grace period can contradict it.
//!
//! **And the markers cross, measured rather than argued.** Decision 2 was taken on the
//! reasoning that markers are ordinary bytes and an SSH channel is a byte pipe, which is a
//! good argument and is not a measurement. Measured 2026-08-29 against `docker/ssh` — Debian
//! bookworm, bash 5.2.15, OpenSSH 9.2 — the remote shell accepted the same line unchanged and
//! the whole cycle came back: the prompt arrives delimited and a command that exits 7 is
//! announced as having failed with 7, which needs the far end's `D;7` to have survived the
//! trip (`tests/ssh_rig.rs`,
//! `the_markers_a_session_sets_itself_up_with_cross_an_ssh_connection`).
//!
//! **What is claimed is measured, and the scope of the measurement is the transport too.**
//! `bash` ends on `0x04` here because B9 sent that byte down a real SSH connection and
//! watched the session close (`tests/ssh_rig.rs`,
//! `the_byte_that_ends_a_bash_session_over_ssh`). That is deliberately *not* generalised to
//! bash over WSL, which `Wsl::eof` still answers `None` for and which roadmap 23.8 is
//! about: the same shell over a different transport is a different cell of the matrix and a
//! different measurement. B5.2 is why this matters — both bytes that "obviously" end a
//! PowerShell session turned out to end nothing and to be echoed as caret text instead.

use acter_core::{ShellFacts, ShellMarkers};

use crate::setup::setup_for;

/// End of transmission, which a line discipline turns into end-of-file for the program
/// reading the terminal.
const EOT: u8 = 0x04;

/// What is known about a shell at the far end of an SSH connection, by the name it gave.
///
/// `None` — a far end that would not answer, or answered with something that is not a shell
/// name — is a real state this product supports: the session works, nothing is claimed
/// about it, and ending it reports "Acter does not know how" rather than guessing at a byte
/// and leaving text on the line.
pub fn over_ssh(name: Option<&str>) -> ShellFacts {
    let setup = setup_for(name);
    ShellFacts {
        // What the setup earns once it has run, and otherwise assumed rather than believed,
        // exactly as `Plain` assumes it: a shell nobody has set up may or may not mark its
        // boundaries, and assuming it does is what makes a session that never marks anything
        // reach `IntegrationUnavailable` — the sentence that tells a listener why nothing is
        // being read aloud (spec B9, decision 2).
        markers: setup
            .as_ref()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full),
        eof: name.and_then(ends_with),
        // Nobody has measured a line-discarding byte for any shell over SSH, and escape is a
        // meta prefix to every POSIX reader — so nothing is written on the strength of what
        // `cmd.exe`'s line editor does with it.
        discards_line: None,
        setup,
    }
}

/// The bytes that end this shell's input, for the shells somebody has actually measured
/// over this transport.
fn ends_with(name: &str) -> Option<Vec<u8>> {
    match name {
        // Measured 2026-08-26 against docker/ssh: `0x04` at an empty prompt closes the
        // channel and the session ends.
        "bash" => Some(vec![EOT]),
        // Everything else is named and nothing more. `dash`, `zsh` and `fish` are all
        // *likely* to end on the same byte for the same reason bash does — the line
        // discipline, not the shell — and "likely" is what this project does not ship.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one shell measured over this transport, and the only one that may claim an
    /// ending.
    #[test]
    fn bash_ends_on_the_byte_that_was_measured() {
        assert_eq!(over_ssh(Some("bash")).eof, Some(vec![0x04]));
    }

    /// **Named, and nothing more.** A shell Acter can identify but has never measured gets
    /// a name for the sentence a listener hears and no claim about how to end it.
    #[test]
    fn a_shell_nobody_measured_is_named_without_being_claimed() {
        for named in ["zsh", "fish", "dash", "sh"] {
            assert_eq!(
                over_ssh(Some(named)).eof,
                None,
                "{named} has not been measured over SSH"
            );
        }
    }

    /// A far end that never answered is the honest absence, not a default.
    #[test]
    fn a_far_end_that_said_nothing_is_claimed_nothing_about() {
        assert_eq!(over_ssh(None).eof, None);
    }

    /// **A far end with no measured setup assumes full markers and therefore says it is
    /// unintegrated.** This is the assertion that stops somebody "helpfully" giving an
    /// unmeasured remote shell a narrower claim — which would produce a session that waits
    /// for boundaries nothing will ever send, and says nothing at all while it waits.
    #[test]
    fn a_far_end_nothing_is_run_in_is_assumed_to_mark_everything() {
        for named in [Some("zsh"), Some("nushell"), None] {
            let facts = over_ssh(named);

            assert_eq!(facts.setup, None, "{named:?} has no measured setup");
            assert_eq!(
                facts.markers,
                ShellMarkers::Full,
                "{named:?} is assumed to mark, so the grace period can report the truth"
            );
        }
    }

    /// **One mechanism for every transport** (spec B9.5, decision 2): a remote bash is set up
    /// with the same line a distribution's bash is, because the setup is a property of the
    /// shell rather than of how Acter reached it.
    #[test]
    fn a_remote_shell_with_a_measured_setup_is_set_up_with_the_shells_own_line() {
        let facts = over_ssh(Some("bash"));

        assert_eq!(
            facts.setup.as_ref().map(|setup| setup.line.as_str()),
            Some(crate::setup::BASH)
        );
        assert_eq!(facts.markers, ShellMarkers::Full);
    }

    /// And a remote `sh` claims what its own line earns, rather than what bash's does.
    #[test]
    fn a_remote_sh_claims_only_the_prompt_boundaries_its_setup_reaches() {
        let facts = over_ssh(Some("sh"));

        assert!(facts.setup.is_some());
        assert_eq!(facts.markers, ShellMarkers::PromptAndCommandLine);
    }
}
