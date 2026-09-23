//! Adapter: whatever shell a WSL distribution runs, reached through `wsl.exe`, behind the
//! `ShellAdapter` port.

pub(crate) mod distributions;
pub(crate) mod login_shell;

use acter_core::{SessionSetup, ShellAdapter, ShellLaunch, ShellMarkers};

use crate::setup::setup_for;

const DISTRIBUTION_FLAG: &str = "-d";

pub struct Wsl {
    program: String,
    /// `None` means whatever WSL calls the default.
    distribution: Option<String>,
    /// `None` means the distribution did not say what its account runs.
    shell: Option<String>,
}

impl Wsl {
    pub fn new(program: impl Into<String>, shell: Option<&str>) -> Self {
        Self {
            program: program.into(),
            distribution: None,
            shell: shell.map(ToOwned::to_owned),
        }
    }

    pub fn in_distribution(
        program: impl Into<String>,
        distribution: impl Into<String>,
        shell: Option<&str>,
    ) -> Self {
        Self {
            program: program.into(),
            distribution: Some(distribution.into()),
            shell: shell.map(ToOwned::to_owned),
        }
    }

    pub fn running(self, shell: Option<&str>) -> Self {
        Self {
            shell: shell.map(ToOwned::to_owned),
            ..self
        }
    }

    pub fn login_shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }
}

impl ShellAdapter for Wsl {
    fn launch(&self) -> ShellLaunch {
        let mut args = Vec::new();
        if let Some(distribution) = &self.distribution {
            args.push(DISTRIBUTION_FLAG.to_owned());
            args.push(distribution.clone());
        }
        ShellLaunch {
            program: self.program.clone(),
            args,
            environment: Vec::new(),
        }
    }

    fn markers(&self) -> ShellMarkers {
        self.setup()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full)
    }

    fn eof(&self) -> Option<Vec<u8>> {
        None
    }

    fn setup(&self) -> Option<SessionSetup> {
        setup_for(self.shell.as_deref())
    }
}

pub fn is_wsl(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    program.eq_ignore_ascii_case("wsl") || program.eq_ignore_ascii_case("wsl.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_under_wsl_has_no_measured_end_of_input_yet() {
        assert_eq!(
            Wsl::new("wsl.exe", Some("bash")).eof(),
            None,
            "no byte is claimed until one is measured against a real distribution"
        );
    }

    #[test]
    fn the_wsl_client_is_recognized_however_it_was_named() {
        for named in ["wsl", "wsl.exe", "WSL.EXE", r"C:\Windows\system32\wsl.exe"] {
            assert!(is_wsl(named), "{named} is the WSL client");
        }
    }

    #[test]
    fn nothing_else_is_claimed() {
        for named in ["cmd.exe", "bash", "pwsh", "wslconfig.exe", "wsl-notify"] {
            assert!(!is_wsl(named), "{named} is not the WSL client");
        }
    }

    #[test]
    fn a_named_distribution_is_pointed_at_with_its_own_argument() {
        let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("bash")).launch();

        assert_eq!(launch.program, "wsl.exe");
        assert_eq!(launch.args, ["-d", "Ubuntu 24.04"]);
    }

    #[test]
    fn the_default_distribution_is_left_to_wsl_rather_than_named() {
        let launch = Wsl::new("wsl.exe", Some("bash")).launch();

        assert!(
            launch.args.is_empty(),
            "no distribution is invented for a session that did not name one"
        );
    }

    #[test]
    fn the_launch_carries_the_client_it_was_named_by() {
        let launch =
            Wsl::in_distribution(r"C:\Windows\system32\wsl.exe", "Debian", Some("bash")).launch();

        assert_eq!(launch.program, r"C:\Windows\system32\wsl.exe");
    }

    #[test]
    fn nothing_is_pushed_into_the_distribution_at_launch() {
        for named in [Some("bash"), Some("sh"), Some("zsh"), None] {
            let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", named).launch();

            assert!(
                launch.environment.is_empty(),
                "{named:?} starts with an empty environment"
            );
            assert_eq!(
                launch.args,
                ["-d", "Ubuntu 24.04"],
                "{named:?} still starts, in the distribution that was named"
            );
        }
    }

    #[test]
    fn the_launch_says_the_same_thing_whatever_the_distribution_runs() {
        assert_eq!(
            Wsl::new("wsl.exe", Some("bash")).launch(),
            Wsl::new("wsl.exe", Some("zsh")).launch()
        );
        assert_eq!(
            Wsl::new("wsl.exe", None).launch(),
            Wsl::new("wsl.exe", Some("bash")).launch()
        );
    }

    #[test]
    fn a_distribution_running_bash_is_set_up_with_bashs_own_line() {
        let setup = Wsl::new("wsl.exe", Some("bash"))
            .setup()
            .expect("bash has a measured setup");

        assert_eq!(setup.line, crate::setup::BASH);
        assert_eq!(setup.markers, ShellMarkers::Full);
        assert_eq!(
            Wsl::new("wsl.exe", Some("bash")).markers(),
            ShellMarkers::Full
        );
    }

    #[test]
    fn a_distribution_running_zsh_is_set_up_with_zshs_own_line() {
        let setup = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("zsh"))
            .setup()
            .expect("zsh has a measured setup since B5.8");

        assert_eq!(setup.line, crate::setup::ZSH);
        assert_eq!(setup.markers, ShellMarkers::Full);
    }

    #[test]
    fn a_distribution_running_sh_claims_only_what_its_setup_earns() {
        let adapter = Wsl::in_distribution("wsl.exe", "docker-desktop", Some("sh"));

        assert_eq!(
            adapter.markers(),
            ShellMarkers::PromptCommandLineAndExitCode
        );
        assert!(!adapter.markers().marks_output_start());
        assert!(adapter.setup().is_some());
    }

    #[test]
    fn a_distribution_running_a_shell_nobody_measured_has_nothing_run_in_it() {
        for named in ["fish", "nu", "ksh"] {
            let adapter = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some(named));

            assert_eq!(adapter.setup(), None, "{named} has no measured setup");
            assert_eq!(
                adapter.markers(),
                ShellMarkers::Full,
                "{named} is claimed optimistically, so the grace period can contradict it"
            );
        }
    }

    #[test]
    fn a_distribution_that_answered_nothing_has_nothing_run_in_it() {
        let adapter = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", None);

        assert_eq!(adapter.setup(), None);
        assert_eq!(adapter.markers(), ShellMarkers::Full);
    }

    #[test]
    fn the_shell_the_distribution_named_is_what_the_session_is_called_with() {
        assert_eq!(
            Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("zsh")).login_shell(),
            Some("zsh")
        );
        assert_eq!(Wsl::new("wsl.exe", None).login_shell(), None);
    }

    #[test]
    fn a_distribution_can_be_told_what_it_runs_after_it_has_been_started() {
        let adapter = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", None).running(Some("bash"));

        assert_eq!(adapter.login_shell(), Some("bash"));
        assert!(adapter.setup().is_some());
        assert_eq!(
            adapter.launch().args,
            ["-d", "Ubuntu 24.04"],
            "and the launch it was started with is the launch it still describes"
        );
    }

    #[test]
    fn no_shell_under_wsl_has_a_measured_end_of_input_whatever_it_is_called() {
        for named in [Some("bash"), Some("zsh"), Some("dash"), None] {
            assert_eq!(
                Wsl::new("wsl.exe", named).eof(),
                None,
                "{named:?} under WSL has no byte measured against it"
            );
        }
    }
}
