//! Adapter: a shell on this Unix machine, started as a login shell — how it is launched,
//! what Acter runs inside it once it is up, and what ends it.
//!
//! **The local counterpart of [`over_ssh`](crate::over_ssh), and it shares that function's
//! rule**: what a shell's name licenses is the setup somebody measured for it, and nothing
//! else (spec B5.5, decision 4). `/bin/zsh` gets zsh's line because zsh's line was measured;
//! `/bin/tcsh` gets nothing, starts anyway, and is named rather than experimented on.
//!
//! **Why it is not [`Plain`](crate::Plain).** `Plain` starts a program and claims nothing,
//! which was the right answer while every local shell Acter knew was Windows'. A shell chosen
//! from `/etc/shells` is different in three measured ways: it is started as a login shell, it
//! has a setup when its name is one of the measured three, and `0x04` ends it.
//!
//! # `-l`, because that is the session a Mac user already has
//!
//! Terminal.app starts the account's shell as a **login** shell, so `/etc/zprofile`,
//! `~/.zprofile` and `~/.bash_profile` run and the `PATH` a user has spent years arranging is
//! the `PATH` they get. A shell started without it works and behaves subtly differently from
//! every other terminal on the machine, which is the kind of difference a listener cannot see
//! and cannot diagnose (spec M2, decision 4).
//!
//! **Measured 2026-09-01**: all seven entries in this Mac's `/etc/shells` — bash, csh, dash,
//! ksh, sh, tcsh and zsh — start under `-l` and reach a prompt. That is worth a measurement
//! rather than an assumption because the flag is not POSIX and `csh` is not `sh`'s family at
//! all.
//!
//! # What was measured about ending one, and about clearing a line
//!
//! **`0x04` ends the session**, measured 2026-09-01 on a real pseudoconsole against bash
//! 3.2.57, zsh 5.9, `/bin/sh` and dash: the byte was written, the shell exited and the child
//! was reaped. That is the same answer B9 measured for bash over SSH, and it is asserted here
//! rather than inherited from there, because the same shell over a different transport is a
//! different cell of the matrix — which is what `Wsl::eof` still answers `None` for.
//!
//! **`0x15` discards the pending line**, measured the same day and the same way in all four:
//! `garbage` typed, `0x15` written, `echo READY` submitted, and what ran was `echo READY`.
//! **It works even in dash, which has no line editor at all** — and that is the explanation
//! rather than a curiosity: `0x15` is the terminal's own `VKILL`, handled by the line
//! discipline below the shell, so it does not depend on readline being there.

use std::path::Path;

use acter_core::{SessionSetup, ShellAdapter, ShellLaunch, ShellMarkers};

use crate::setup::setup_for;

/// The flag that makes this a login shell, which is what Terminal.app starts.
const LOGIN: &str = "-l";

/// End of transmission, which the line discipline turns into end-of-file for the shell.
const EOT: u8 = 0x04;

/// Kill line — the terminal's own `VKILL`, handled below the shell rather than by it.
const KILL_LINE: u8 = 0x15;

/// One shell on this machine, started the way a login starts it.
pub struct UnixShell {
    /// The file itself, as the connect list resolved it — **the path rather than a name**,
    /// so the file that was verified is the file that is started (spec B5.7, decision 1).
    program: String,
}

impl UnixShell {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// What this shell is called, which is the file's own name and what the setup is keyed
    /// by.
    ///
    /// **The name is guessed from the path and the setup never is** (spec B5.5, decision 4).
    /// `/bin/zsh` is zsh in the sense that matters here — it is what the user calls it and
    /// what `/etc/shells` offered — and what that earns is the line somebody measured for
    /// zsh, not a line assembled from the shape of another shell's.
    fn name(&self) -> Option<String> {
        Path::new(&self.program)
            .file_name()
            .map(|file| file.to_string_lossy().into_owned())
    }
}

impl ShellAdapter for UnixShell {
    /// The file, and `-l`.
    ///
    /// **The environment is empty, as every shell's has been since B9.5** (decision 1).
    /// Nothing is armed at launch: what makes this far end mark its boundaries is a line sent
    /// into the session once it is up, which is the only ordering in which the user's own
    /// startup files do not get the last word.
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: vec![LOGIN.to_owned()],
            environment: Vec::new(),
        }
    }

    /// What this shell will be able to mark **once the setup has run**, and the optimistic
    /// default when there is no setup to run.
    ///
    /// `Full` for a shell nothing is set up in is deliberate and unchanged: it is a claim
    /// rather than a report, and it is the claim the startup grace period exists to
    /// contradict. A tcsh session that never marks anything reaches `IntegrationUnavailable`
    /// and says so, where answering something narrower would leave a listener waiting for
    /// boundaries in silence.
    fn markers(&self) -> ShellMarkers {
        self.setup()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full)
    }

    /// `0x04`, measured rather than assumed (see this module's own notes).
    ///
    /// **Answered for every shell here, including the ones with no setup.** Ending a session
    /// is the line discipline's job rather than the shell's, so it is the one answer that does
    /// not wait on somebody measuring that particular shell's prompt — and a tcsh session
    /// that can be ended is strictly better than one that reports "Acter does not know how".
    fn eof(&self) -> Option<Vec<u8>> {
        Some(vec![EOT])
    }

    /// `0x15`, for [`Self::eof`]'s reason: it is the terminal's, not the shell's.
    fn discards_line(&self) -> Option<u8> {
        Some(KILL_LINE)
    }

    /// What Acter runs inside the session once it is up, for the shell this file is.
    ///
    /// **The same line a WSL bash and an SSH bash get** (spec B9.5, decision 2), which is the
    /// point of keying a setup by the shell rather than by the transport: bash 3.2.57 on a Mac
    /// and bash 5.2 over SSH are set up by one program, and M2's job was to measure that the
    /// one program works on the old one rather than to write a second.
    fn setup(&self) -> Option<SessionSetup> {
        setup_for(self.name().as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_is_started_as_a_login_shell_and_as_the_file_it_is() {
        let launch = UnixShell::new("/bin/zsh").launch();

        assert_eq!(launch.program, "/bin/zsh", "the file, not the name");
        assert_eq!(launch.args, ["-l"], "which is what Terminal.app starts");
        assert!(
            launch.environment.is_empty(),
            "nothing is armed at launch, as of B9.5"
        );
    }

    /// **The three shells somebody measured get their own lines**, and each is the line that
    /// was measured for it rather than one adapted from a neighbour.
    #[test]
    fn a_measured_shell_gets_the_line_that_was_measured_for_it() {
        for (program, expected) in [
            ("/bin/zsh", setup_for(Some("zsh"))),
            ("/bin/bash", setup_for(Some("bash"))),
            ("/bin/sh", setup_for(Some("sh"))),
            ("/bin/dash", setup_for(Some("dash"))),
        ] {
            let setup = UnixShell::new(program).setup();

            assert_eq!(setup, expected, "{program} runs its own shell's line");
            assert!(setup.is_some(), "{program} has a measured setup");
        }
    }

    /// zsh's line is not bash's, which is the rule B5.8 paid for and this must not undo: a
    /// path that selected one adapter for "a Unix shell" and then ran one program in all of
    /// them would be exactly the guess B5.5 forbids.
    #[test]
    fn two_measured_shells_do_not_share_a_line() {
        let zsh = UnixShell::new("/bin/zsh").setup().expect("zsh is measured");
        let bash = UnixShell::new("/bin/bash")
            .setup()
            .expect("bash is measured");

        assert_ne!(zsh.line, bash.line);
    }

    /// **A shell nobody measured is started and named, never experimented on** (spec B5.5,
    /// decision 4). It is offered in the panel like any other — it is in `/etc/shells`, so
    /// this machine says an account may log in to it — and what it does not get is a line.
    #[test]
    fn a_shell_nobody_measured_starts_with_nothing_run_inside_it() {
        for program in ["/bin/tcsh", "/bin/csh", "/bin/ksh"] {
            let shell = UnixShell::new(program);

            assert_eq!(shell.setup(), None, "{program} has nothing measured for it");
            assert_eq!(
                shell.markers(),
                ShellMarkers::Full,
                "{program} claims the optimistic default, which the grace period contradicts"
            );
            assert_eq!(shell.launch().args, ["-l"], "{program} still starts");
        }
    }

    /// The marker claim follows the setup rather than the name: `sh` reaches the prompt
    /// boundaries and the exit code and says so, where zsh claims the lot.
    #[test]
    fn what_a_shell_claims_is_what_its_own_line_earns() {
        assert_eq!(UnixShell::new("/bin/zsh").markers(), ShellMarkers::Full);
        assert_eq!(
            UnixShell::new("/bin/sh").markers(),
            ShellMarkers::PromptCommandLineAndExitCode,
            "sh has no hook for where output begins, and the tracker supplies it"
        );
    }

    /// Ending a session and clearing a line are the terminal's answers, so every shell here
    /// has them — including the ones with no setup.
    #[test]
    fn every_shell_here_can_be_ended_and_can_have_its_line_cleared() {
        for program in ["/bin/zsh", "/bin/bash", "/bin/sh", "/bin/tcsh"] {
            let shell = UnixShell::new(program);

            assert_eq!(shell.eof(), Some(vec![0x04]), "{program} ends on EOT");
            assert_eq!(
                shell.discards_line(),
                Some(0x15),
                "{program}'s pending line is killed by the line discipline"
            );
        }
    }

    /// A path with no file name at all does not crash and claims nothing: it is the same
    /// state as a shell nobody has measured.
    #[test]
    fn a_program_with_no_name_claims_nothing_about_itself() {
        let shell = UnixShell::new("/");

        assert_eq!(shell.setup(), None);
        assert_eq!(shell.markers(), ShellMarkers::Full);
    }
}
