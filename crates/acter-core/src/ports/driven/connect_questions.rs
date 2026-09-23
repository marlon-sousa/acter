//! Port (driven): everything one attempt to connect may have to ask the person in front of
//! the window, before there is a session to ask it in.
//!
//! Every method blocks until somebody answers; see ssh_questions.rs for where it may be called.

use crate::{SessionSetup, SshQuestions, Verdict};

pub trait ConnectQuestions: SshQuestions {
    /// An implementer with nobody to ask answers `DoNotStart`.
    fn unverified(&self, question: ProgramQuestion) -> ProgramAnswer;

    fn set_up_session(&self, question: SetupQuestion) -> SetupAnswer;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupQuestion {
    pub shell: String,
    pub setup: SessionSetup,
}

impl SetupQuestion {
    pub fn detected(&self) -> String {
        format!("Acter has detected that this session runs {}.", self.shell)
    }

    pub fn offer(&self) -> String {
        // Every block is headed by the shell's echo with or without setup, so never promise
        // headings here.
        let gained = if self.setup.markers.reports_exit_code() {
            "You are told when a command fails, and Acter can tell when each command has \
             finished."
        } else {
            "Acter can tell when each command has finished. It cannot yet tell you when a \
             command fails in this shell."
        };
        format!("Acter can set it up so it tells you more about what you run. {gained}")
    }

    pub fn command(&self) -> &str {
        &self.setup.line
    }
}

pub const IF_YOU_SKIP: &str = "If you skip this, the session still works. You will hear what \
                               commands print here, but not whether they worked.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAnswer {
    SetUp { remember: bool },
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramQuestion {
    pub label: String,
    pub program: String,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramAnswer {
    Start,
    DoNotStart,
}

impl ConnectQuestions for crate::Unasked {
    fn unverified(&self, _question: ProgramQuestion) -> ProgramAnswer {
        ProgramAnswer::DoNotStart
    }

    fn set_up_session(&self, _question: SetupQuestion) -> SetupAnswer {
        SetupAnswer::Skip
    }
}

#[cfg(test)]
mod tests {
    use crate::{Fault, ShellMarkers, Unasked};

    use super::*;

    #[test]
    fn with_nobody_to_ask_nothing_unverified_is_started() {
        let answer = Unasked.unverified(ProgramQuestion {
            label: "PowerShell 7".to_owned(),
            program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
            verdict: Verdict::Untrusted {
                fault: Fault::NotSigned,
            },
        });

        assert_eq!(answer, ProgramAnswer::DoNotStart);
    }

    fn bash() -> SetupQuestion {
        SetupQuestion {
            shell: "bash".to_owned(),
            setup: SessionSetup {
                line: "__acter_status=$?".to_owned(),
                markers: ShellMarkers::Full,
            },
        }
    }

    #[test]
    fn with_nobody_to_ask_no_session_is_set_up() {
        assert_eq!(Unasked.set_up_session(bash()), SetupAnswer::Skip);
    }

    #[test]
    fn the_question_names_the_shell_that_was_detected() {
        assert_eq!(
            bash().detected(),
            "Acter has detected that this session runs bash."
        );
    }

    #[test]
    fn a_shell_that_marks_everything_offers_endings_and_failures() {
        let offer = bash().offer();

        assert!(offer.contains("when each command has finished"), "{offer}");
        assert!(offer.contains("told when a command fails"), "{offer}");
        assert!(
            !offer.contains("heading"),
            "a heading is not what setting a session up buys: {offer}"
        );
        assert!(
            !offer.contains("shell integration"),
            "the phrase A13 removed is not said to a listener: {offer}"
        );
    }

    #[test]
    fn a_shell_that_says_how_a_command_went_is_offered_failures_too() {
        let question = SetupQuestion {
            shell: "sh".to_owned(),
            setup: SessionSetup {
                line: "PS1=...".to_owned(),
                markers: ShellMarkers::PromptCommandLineAndExitCode,
            },
        };
        let offer = question.offer();

        assert!(offer.contains("when each command has finished"), "{offer}");
        assert!(offer.contains("told when a command fails"), "{offer}");
    }

    #[test]
    fn a_shell_that_marks_only_its_prompt_says_what_it_cannot_do() {
        let question = SetupQuestion {
            shell: "cmd".to_owned(),
            setup: SessionSetup {
                line: "PS1=...".to_owned(),
                markers: ShellMarkers::PromptAndCommandLine,
            },
        };
        let offer = question.offer();

        assert!(offer.contains("when each command has finished"), "{offer}");
        assert!(
            offer.contains("cannot yet tell you when a command fails"),
            "{offer}"
        );
    }

    #[test]
    fn the_command_is_carried_exactly_as_it_would_run() {
        assert_eq!(bash().command(), "__acter_status=$?");
    }

    #[test]
    fn the_refusal_says_what_still_works_before_what_does_not() {
        assert!(IF_YOU_SKIP.starts_with("If you skip this, the session still works."));
        assert!(
            IF_YOU_SKIP
                .ends_with("You will hear what commands print here, but not whether they worked.")
        );
    }
}
