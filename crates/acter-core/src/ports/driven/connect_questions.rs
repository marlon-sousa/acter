//! Port (driven): everything one attempt to connect may have to ask the person in front of
//! the window, before there is a session to ask it in.
//!
//! **It is [`SshQuestions`] plus the two questions that are not about a far end.** B9
//! established the shape — a question goes out on the conversation's channel and the
//! connection parks until an answer comes back — for two things a server can stop on: a host
//! key nobody has seen, and a password nobody has typed. B5.7 adds a third, and it is asked
//! about this machine rather than about the other one: the file that is about to be started
//! did not verify, and starting it is a decision only the user can make (spec B5.7,
//! decision 6).
//!
//! **B9.5 adds a fourth, and it is the first that is not a warning.** The connection has
//! succeeded, the far end has said what shell it runs, and Acter is about to run one command
//! inside the session so that a listener gets a heading for each command and is told when one
//! fails. The command is shown before it runs, verbatim (spec B9.5, decision 9). It arrives
//! on this seam for the reason the other two do — it is asked after the connection succeeds
//! and before anything is sent, which is exactly the window this port exists for — and it
//! inherits both properties the other two established: refusing is a decision rather than a
//! failure, and nothing runs that nobody said yes to.
//!
//! **A supertrait rather than a fourth method on `SshQuestions`.** The SSH transport is
//! handed an asker and must not be handed a question it can never ask: what it needs is the
//! two questions a server raises, and it is measured against a real server with no window
//! anywhere near it. What the *connect service* needs is all three, so it takes this and
//! passes the SSH half down.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. Blocking is the point
//! of it, for [`SshQuestions`]' reason: the thing that waits is a task of the connection's
//! own, never an invoke, which Tauri runs on the main thread and which would deadlock the
//! answer it is waiting for.

use crate::{SessionSetup, SshQuestions, Verdict};

/// The questions an attempt to connect asks, of which SSH's two are the first.
pub trait ConnectQuestions: SshQuestions {
    /// The file this machine is about to start did not verify, and here is what was found.
    ///
    /// **Never a gate, and the default is not to start** (decision 6). Everything this
    /// machine has stays in the list — a self-built pwsh, a corporate re-signed build, a
    /// damaged catalog database and an offline revocation check are all legitimate and all
    /// common — and what changes is that the user is told before the program runs rather
    /// than after. The safe answer is the one that does nothing, for the reason
    /// [`SshQuestions::host_key`] refuses by default.
    fn unverified(&self, question: ProgramQuestion) -> ProgramAnswer;

    /// The session is up, this is what it runs, and this is the command Acter would run
    /// inside it.
    ///
    /// **Asked once per shell per person** (spec B9.5, decision 10), which is what
    /// [`Explained`](crate::Explained) remembers; and asked at all only because the Connect
    /// dialog's checkbox said so, which is what [`SetUp`](crate::SetUp) carries. The two are
    /// not the same thing and storing them together is the mistake this shape avoids.
    ///
    /// **Unlike the other three, the default here is to go ahead.** A host key and an
    /// unverified file are security decisions where the safe answer is the one that does
    /// nothing; this is Acter offering to tell a listener more about their own session, and
    /// the thing it defends against is surprise rather than harm. What makes "on by default"
    /// honest is that the command is disclosed before it runs and left in the buffer and in
    /// the shell's history afterwards.
    fn set_up_session(&self, question: SetupQuestion) -> SetupAnswer;
}

/// What a person is told before Acter runs anything in their session.
///
/// **Every field here becomes speech, and the words are the domain's** — the same rule
/// [`ProgramQuestion`] follows. What a listener is entitled to hear is decided once, here,
/// and a dialog renders it rather than composing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupQuestion {
    /// What the far end said it runs, as the probe read it: `bash`, `sh`, `zsh`.
    pub shell: String,
    /// The setup that would run, which carries both the command and how far the markers it
    /// earns reach.
    pub setup: SessionSetup,
}

impl SetupQuestion {
    /// What was detected, said plainly and with a subject.
    ///
    /// **The dialog names the shell it detected**, which is why the probe stays ahead of it
    /// (spec B9.5, decision 14). A dialog that said "this session" and nothing more would be
    /// asking somebody to authorise a command against a far end it would not name.
    pub fn detected(&self) -> String {
        format!("Acter has detected that this session runs {}.", self.shell)
    }

    /// What the person gets for saying yes, in their words rather than this project's.
    ///
    /// **Not "shell integration"** — A13 removed that phrase from everything a listener hears
    /// because it is vocabulary a user does not have. What they get is a heading for each
    /// command and being told when one fails, and for a shell that cannot say how a command
    /// ended the second half is missing and is said to be missing (spec B9.5, decision 8).
    /// Saying "partly" is the whole reason the per-shell answer is a
    /// [`ShellMarkers`](crate::ShellMarkers) rather than a yes.
    ///
    /// **And POSIX `sh` stopped being the shell that needed it** (roadmap 23.15): it reports
    /// exit codes, so it gets the sentence bash gets. The half-sentence stays because the
    /// question it answers is real — a shell with only a prompt is where `sh` was believed to
    /// be — and because promising a verdict that never arrives is the one thing this dialog
    /// must not do.
    pub fn offer(&self) -> String {
        // **What a listener gets follows from the verdict, not from the full cycle** (roadmap
        // 23.15). Where output begins is a boundary the tracker can supply for itself; whether
        // a command worked is the one thing no amount of inference can recover, so it is the
        // one thing this sentence has to be careful about.
        let gained = if self.setup.markers.reports_exit_code() {
            "You get a heading for each command, and you are told when a command fails."
        } else {
            "You get a heading for each command. Acter cannot yet tell you when a command \
             fails in this shell."
        };
        format!("Acter can set it up so it tells you more about what you run. {gained}")
    }

    /// The command, verbatim, for a field it can be read out of character by character.
    pub fn command(&self) -> &str {
        &self.setup.line
    }
}

/// What refusing costs, which is A13's shipped sentence with what still works in front of it.
///
/// **It is the register test rather than a placeholder** (spec B9.5, decision 9). If the
/// refusal reads in the same voice as the help topic F1 opens, the dialog is in the user's
/// words; if it does not, something in this dialog is speaking this project's.
pub const IF_YOU_CANCEL: &str = "If you cancel, the session still works. You will hear what \
                                 commands print here, but not whether they worked.";

/// What the person decided about setting this session up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAnswer {
    /// Run it.
    SetUp {
        /// Whether they ticked "do not show this dialog again", which is remembered for this
        /// shell and for no other.
        remember: bool,
    },
    /// Do not, this session. The session still works and says what it will and will not tell
    /// them; the Connect dialog's checkbox is what refuses durably (decision 9).
    Skip,
}

/// What a person is asked about a file that would not verify, in the order it has to be
/// said.
///
/// **Every field here becomes speech.** The verdict arrives whole rather than as a sentence
/// somebody assembled at the call site, so the words a listener hears are decided in one
/// place — [`Verdict::said`] — and the dialog can also put the path and the signer somewhere
/// they can be read character by character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramQuestion {
    /// What the user chose, as they heard it in the list: `PowerShell 7 (Microsoft Store)`.
    pub label: String,
    /// The file that would be started, in full. **The full path rather than the name**,
    /// because the thing this check defeats is `PATH`-order hijacking, and which directory
    /// the file is in is the whole of what a user needs to recognise it as wrong.
    pub program: String,
    /// What verifying it found.
    pub verdict: Verdict,
}

/// What the person decided about starting it.
///
/// A two-variant enum rather than a `bool`, for [`HostKeyAnswer`](crate::HostKeyAnswer)'s
/// reason: `true` at a call site three files away does not say which way it went, and this
/// is a security decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramAnswer {
    /// Start it anyway. The attempt goes on, and what was agreed to is said out loud.
    Start,
    /// Do not. The attempt ends and says why, and whatever session was running is untouched.
    DoNotStart,
}

/// Nobody to ask, so nothing unverified is started and nothing is run inside a session.
///
/// The same refusal [`Unasked`](crate::Unasked) makes about a host key, and for the same
/// reason: a launch that names a profile from the environment has no window to put a
/// question in, and starting a file nobody could be asked about is the "accept everything"
/// mode Acter does not have.
///
/// **The setup question refuses for a reason that survives its friendlier subject** (spec
/// B9.5, decision 9): the checkbox authorises and the dialog discloses, and a launch from the
/// environment has neither. Setting a session up behind nobody would be running a command in
/// somebody's shell that nothing ever showed them.
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

    /// The null asker's one behaviour, and the one that matters: with nobody to ask, nothing
    /// unverified starts. A `Start` here would mean a launch from the environment quietly
    /// running whatever `PATH` resolved to.
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

    /// **The checkbox authorises and the dialog discloses, and a launch from the environment
    /// has neither** (spec B9.5, decision 9). So nothing is run, even though nothing here is
    /// dangerous: what would be missing is the disclosure, not the safety.
    #[test]
    fn with_nobody_to_ask_no_session_is_set_up() {
        assert_eq!(Unasked.set_up_session(bash()), SetupAnswer::Skip);
    }

    /// The dialog names the shell it detected, which is why the probe stays ahead of it.
    #[test]
    fn the_question_names_the_shell_that_was_detected() {
        assert_eq!(
            bash().detected(),
            "Acter has detected that this session runs bash."
        );
    }

    /// **What a listener gets, in their words** — and none of the phrase A13 removed.
    #[test]
    fn a_shell_that_marks_everything_offers_headings_and_failures() {
        let offer = bash().offer();

        assert!(offer.contains("a heading for each command"), "{offer}");
        assert!(offer.contains("told when a command fails"), "{offer}");
        assert!(
            !offer.contains("shell integration"),
            "the phrase A13 removed is not said to a listener: {offer}"
        );
    }

    /// **A shell that reports exit codes gets the sentence bash gets** (roadmap 23.15), and
    /// the marker set it gets there by is not this sentence's business: where output begins
    /// is a boundary the tracker supplies for itself, and a listener is never told about it.
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

        assert!(offer.contains("a heading for each command"), "{offer}");
        assert!(offer.contains("told when a command fails"), "{offer}");
    }

    /// **The sentence has to be able to say "partly"** (spec B9.5, decision 8). A shell that
    /// reaches the prompt boundaries and no further cannot say whether a command worked, and a
    /// dialog that promised failures there would be promising something the setup cannot
    /// deliver. POSIX `sh` was that shell until roadmap 23.15 measured otherwise; `cmd.exe`
    /// still is.
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

        assert!(offer.contains("a heading for each command"), "{offer}");
        assert!(
            offer.contains("cannot yet tell you when a command fails"),
            "{offer}"
        );
    }

    /// The command is shown verbatim, because that is the disclosure the whole dialog is
    /// (spec B9.5, decision 3).
    #[test]
    fn the_command_is_carried_exactly_as_it_would_run() {
        assert_eq!(bash().command(), "__acter_status=$?");
    }

    /// **The register test, pinned** (spec B9.5, decision 9): the closing line is A13's
    /// shipped sentence, so the refusal reads in the same voice as the help topic F1 opens.
    #[test]
    fn the_refusal_says_what_still_works_before_what_does_not() {
        assert!(IF_YOU_CANCEL.starts_with("If you cancel, the session still works."));
        assert!(
            IF_YOU_CANCEL
                .ends_with("You will hear what commands print here, but not whether they worked.")
        );
    }
}
