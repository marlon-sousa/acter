//! Adapter: a shell on this Unix machine, started as a login shell.

use std::path::Path;

use acter_core::{SessionSetup, ShellAdapter, ShellLaunch, ShellMarkers};

use crate::setup::setup_for;

/// All seven shells a stock macOS `/etc/shells` lists (bash, csh, dash, ksh, sh, tcsh, zsh)
/// start under `-l` and reach a prompt.
const LOGIN: &str = "-l";

/// Ends the session in bash 3.2.57, zsh 5.9, `/bin/sh` and dash on a macOS pseudoconsole.
const EOT: u8 = 0x04;

/// Discards the pending line in those same four shells, dash included, because the line
/// discipline's `VKILL` handles it below the shell.
const KILL_LINE: u8 = 0x15;

pub struct UnixShell {
    program: String,
}

impl UnixShell {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn name(&self) -> Option<String> {
        Path::new(&self.program)
            .file_name()
            .map(|file| file.to_string_lossy().into_owned())
    }
}

impl ShellAdapter for UnixShell {
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: vec![LOGIN.to_owned()],
            environment: Vec::new(),
        }
    }

    fn markers(&self) -> ShellMarkers {
        self.setup()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full)
    }

    fn eof(&self) -> Option<Vec<u8>> {
        Some(vec![EOT])
    }

    fn discards_line(&self) -> Option<u8> {
        Some(KILL_LINE)
    }

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

    #[test]
    fn two_measured_shells_do_not_share_a_line() {
        let zsh = UnixShell::new("/bin/zsh").setup().expect("zsh is measured");
        let bash = UnixShell::new("/bin/bash")
            .setup()
            .expect("bash is measured");

        assert_ne!(zsh.line, bash.line);
    }

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

    #[test]
    fn what_a_shell_claims_is_what_its_own_line_earns() {
        assert_eq!(UnixShell::new("/bin/zsh").markers(), ShellMarkers::Full);
        assert_eq!(
            UnixShell::new("/bin/sh").markers(),
            ShellMarkers::PromptCommandLineAndExitCode,
            "sh has no hook for where output begins, and the tracker supplies it"
        );
    }

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

    #[test]
    fn a_program_with_no_name_claims_nothing_about_itself() {
        let shell = UnixShell::new("/");

        assert_eq!(shell.setup(), None);
        assert_eq!(shell.markers(), ShellMarkers::Full);
    }
}
