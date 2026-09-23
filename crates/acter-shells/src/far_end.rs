//! Policy: what a shell's *name* licenses Acter to believe about it, when Acter did not
//! start that shell and cannot inject anything into it.

use acter_core::{ShellFacts, ShellMarkers};

use crate::setup::setup_for;

const EOT: u8 = 0x04;

/// `None`, or a name that is not a shell, yields facts that claim no setup and no ending.
pub fn over_ssh(name: Option<&str>) -> ShellFacts {
    let setup = setup_for(name);
    ShellFacts {
        // Assumed rather than measured when there is no setup, as `Plain::markers` assumes it.
        markers: setup
            .as_ref()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full),
        eof: name.and_then(ends_with),
        // Escape is a meta prefix to a POSIX line editor, not a line discard.
        discards_line: None,
        setup,
    }
}

/// None for any shell whose ending has not been measured over SSH; for bash and zsh see the
/// `the_byte_that_ends_a_*_session_over_ssh` tests in crates/acter-transports/tests/ssh_rig.rs.
fn ends_with(name: &str) -> Option<Vec<u8>> {
    match name {
        "bash" => Some(vec![EOT]),
        "zsh" => Some(vec![EOT]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shells_that_were_measured_end_on_the_byte_that_was_measured() {
        assert_eq!(over_ssh(Some("bash")).eof, Some(vec![0x04]));
        assert_eq!(
            over_ssh(Some("zsh")).eof,
            Some(vec![0x04]),
            "measured in B5.8"
        );
    }

    #[test]
    fn a_shell_nobody_measured_is_named_without_being_claimed() {
        for named in ["fish", "dash", "sh", "ksh"] {
            assert_eq!(
                over_ssh(Some(named)).eof,
                None,
                "{named} has not been measured over SSH"
            );
        }
    }

    #[test]
    fn a_far_end_that_said_nothing_is_claimed_nothing_about() {
        assert_eq!(over_ssh(None).eof, None);
    }

    #[test]
    fn a_far_end_nothing_is_run_in_is_assumed_to_mark_everything() {
        for named in [Some("fish"), Some("nushell"), None] {
            let facts = over_ssh(named);

            assert_eq!(facts.setup, None, "{named:?} has no measured setup");
            assert_eq!(
                facts.markers,
                ShellMarkers::Full,
                "{named:?} is assumed to mark, so the grace period can report the truth"
            );
        }
    }

    #[test]
    fn a_remote_shell_with_a_measured_setup_is_set_up_with_the_shells_own_line() {
        let facts = over_ssh(Some("bash"));

        assert_eq!(
            facts.setup.as_ref().map(|setup| setup.line.as_str()),
            Some(crate::setup::BASH)
        );
        assert_eq!(facts.markers, ShellMarkers::Full);
    }

    #[test]
    fn a_remote_sh_claims_the_prompt_boundaries_and_a_verdict() {
        let facts = over_ssh(Some("sh"));

        assert!(facts.setup.is_some());
        assert_eq!(facts.markers, ShellMarkers::PromptCommandLineAndExitCode);
    }

    #[test]
    fn a_remote_zsh_is_set_up_with_the_line_measured_for_zsh() {
        let facts = over_ssh(Some("zsh"));

        assert_eq!(
            facts.setup.as_ref().map(|setup| setup.line.as_str()),
            Some(crate::setup::ZSH)
        );
        assert_eq!(facts.markers, ShellMarkers::Full);
    }
}
