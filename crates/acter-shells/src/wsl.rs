//! Adapter: whatever shell a WSL distribution runs, reached through `wsl.exe`, behind the
//! `ShellAdapter` port — how the client is started, which distribution it is pointed at, and
//! what Acter runs inside the session once it is up.
//!
//! **The first far end Acter sets up that is not in its own process tree.** `wsl.exe` is a
//! client to a shell in another kernel, exactly as `docker.exe` is (ROADMAP 22.6). An
//! interrupt does cross, because it travels as data: `0x03` written to the pseudoconsole
//! reaches bash's own line discipline, which turns it into `SIGINT` — measured by 22.6, and
//! re-measured on 2026-08-24 as the exit code `130` arriving in a `D` marker.
//!
//! **The one adapter whose variants are discovered rather than known.** `Cmd` is the same
//! shell on every Windows machine; which distributions exist is the same answer on no two
//! machines at all, so *that* question is I/O and lives behind
//! [`InstalledShells`](acter_core::InstalledShells) rather than here. This module knows only
//! how to start one once its name is in hand.
//!
//! **Since B5.5, which shell it starts is discovered too**, because `wsl.exe` runs whatever
//! login shell the distribution's account carries in its own passwd entry. This adapter is
//! *told* what that shell is rather than assuming bash, and [`login_shell`] is what its answer
//! means. What it is not is a gate: a distribution that would not answer starts anyway, with
//! nothing claimed about it.
//!
//! **Since B9.5 the launch carries nothing but the client and `-d`.** Everything Acter used to
//! arm here rode in the environment `wsl.exe` was started with — a `PROMPT_COMMAND` and the
//! `WSLENV` entry that carried it across the kernel boundary — which meant bash inherited it
//! *before* it sourced `.bashrc`, and every startup file the user owns got the last word after
//! it. 23.11 measured what that costs: three separate dotfile shapes that keep markers flowing
//! while corrupting them, one of which made Acter announce that a failed command had
//! succeeded. What replaces it is a line sent into the session once it is established, which
//! is [`setup`](crate::setup)'s and is shared with SSH rather than being WSL's.
//!
//! How a distribution list is read out of `wsl.exe -l -q` is in [`distributions`].

pub(crate) mod distributions;
pub(crate) mod login_shell;

use acter_core::{SessionSetup, ShellAdapter, ShellLaunch, ShellMarkers};

use crate::setup::setup_for;

/// The `-d` flag that points `wsl.exe` at one distribution rather than at the default.
const DISTRIBUTION_FLAG: &str = "-d";

/// A session inside one WSL distribution, with whatever shell that distribution runs.
pub struct Wsl {
    /// The client program as the user named it, for [`Cmd`](crate::Cmd)'s reason: `wsl`,
    /// `wsl.exe` and a full path to it are the same client and which of them reaches the
    /// transport is the user's business.
    program: String,
    /// Which distribution to start in, or `None` for whatever WSL calls the default.
    ///
    /// A `String` and not a `&'static str`, which is the reason
    /// [`ShellLaunch`](acter_core::ShellLaunch) is owned rather than borrowed: this name
    /// is read off a running `wsl.exe` at the moment a user opens the connect list.
    distribution: Option<String>,
    /// What that distribution's account actually runs, as
    /// [`InstalledShells::login_shell`](acter_core::InstalledShells::login_shell) answered,
    /// and `None` when nothing answered.
    ///
    /// **Carried rather than assumed, since B5.5.** Before that the field did not exist and
    /// bash was the silent answer everywhere, including in the accounts that do not run it.
    shell: Option<String>,
}

impl Wsl {
    /// A session in whatever distribution WSL considers the default — `wsl.exe` with no
    /// `-d` at all.
    ///
    /// Not a guess at which one that is: asking WSL to choose is a different thing from
    /// this program choosing, and only the first of the two is ever right after the user
    /// changes their default. Measured on 2026-08-24 as starting the same integrated bash
    /// as a named distribution does.
    pub fn new(program: impl Into<String>, shell: Option<&str>) -> Self {
        Self {
            program: program.into(),
            distribution: None,
            shell: shell.map(ToOwned::to_owned),
        }
    }

    /// A session in the distribution with this name, as `wsl.exe -l -q` spelled it.
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

    /// The same session, once the distribution has said what it runs.
    ///
    /// **It exists because the probe no longer has to run in front of the launch** (spec
    /// B9.5, decision 14). What is injected used to be part of `ShellLaunch`, so the decision
    /// whether to inject could not be made after the client had started; nothing is injected
    /// now, so the client can be started while the probe is still being answered and the
    /// answer applied to the adapter afterwards. What still has to precede the *session* is
    /// this answer, because the dialog names the shell it detected.
    pub fn running(self, shell: Option<&str>) -> Self {
        Self {
            shell: shell.map(ToOwned::to_owned),
            ..self
        }
    }

    /// What this session is with, for the sentence a listener hears, and `None` when the
    /// distribution said nothing this program is willing to say out loud.
    pub fn login_shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }
}

impl ShellAdapter for Wsl {
    /// The client and `-d`, and nothing else.
    ///
    /// **The environment is empty and that is the point of B9.5** (decision 1). It used to
    /// carry the marker program and the `WSLENV` entry that let it cross the kernel boundary,
    /// which is exactly the ordering that let an ordinary dotfile beat Acter to the prompt.
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

    /// What this far end will be able to mark **once the setup has run**, and the optimistic
    /// default when there is no setup to run.
    ///
    /// `Full` for a distribution nothing is set up in is deliberate and unchanged (spec B5.5,
    /// decision 4, following B9's decision 2): it is a claim rather than a report, and it is
    /// the claim the startup grace period exists to contradict. A session that never marks
    /// anything reaches `IntegrationUnavailable` and says so, where answering something
    /// narrower would leave a listener waiting for boundaries in silence.
    ///
    /// A shell whose setup reaches only the prompt boundaries says so instead, which is what
    /// `sh` is (spec B9.5, decision 8) — and saying it here rather than discovering it later
    /// is what keeps the tracker's belief a construction argument.
    fn markers(&self) -> ShellMarkers {
        self.setup()
            .map(|setup| setup.markers)
            .unwrap_or(ShellMarkers::Full)
    }

    /// **Nobody has measured what ends a session through this transport, so this says so
    /// rather than guessing.**
    ///
    /// `0x04` is the obvious answer and it is probably right: the shell reads from a
    /// pseudoconsole whose line discipline turns that byte into end-of-file. But "probably
    /// right" is exactly what B5.2 measured and disproved for the shell next door — *neither*
    /// control byte that is supposed to end a PowerShell session does, both are echoed as
    /// caret text, and a line submitted behind one runs as a command the user never typed.
    ///
    /// `None` means "Acter does not know how to end this shell", which the session reports
    /// out loud. It becomes a byte the day somebody drives a real distribution and watches
    /// the session close.
    fn eof(&self) -> Option<Vec<u8>> {
        None
    }

    /// What Acter runs inside the session once it is up, for the shell this distribution
    /// actually answered with.
    ///
    /// **Per shell rather than per transport, which is the whole of B9.5's decision 2**: this
    /// is the same answer `far_end::over_ssh` gives for the same name, so one WSL bash and one
    /// SSH bash share a setup for the first time.
    fn setup(&self) -> Option<SessionSetup> {
        setup_for(self.shell.as_deref())
    }
}

/// Whether this program is the WSL client, however it was named.
///
/// Case-insensitive and extension-insensitive for [`is_cmd`](crate::cmd) reasons: `wsl`,
/// `wsl.exe` and a full path to it are all the same client and a user typing any of them
/// means the same thing.
pub fn is_wsl(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    program.eq_ignore_ascii_case("wsl") || program.eq_ignore_ascii_case("wsl.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The absence is the assertion.** Answering `None` is a claim that Acter does not
    /// know how to end a bash session through this transport, and it is deliberate rather
    /// than forgotten: `0x04` is the obvious byte, and B5.2 measured the equivalent
    /// assumption to be wrong for PowerShell. This test exists so that supplying a byte
    /// here is a decision somebody makes on purpose, with a measurement behind it, rather
    /// than a plausible edit nobody notices.
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

    /// The distribution name is the only thing `-d` exists to carry, and it reaches the
    /// transport as its own argument rather than glued to the flag: a name with a space in
    /// it would otherwise become two arguments and start nothing.
    #[test]
    fn a_named_distribution_is_pointed_at_with_its_own_argument() {
        let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("bash")).launch();

        assert_eq!(launch.program, "wsl.exe");
        assert_eq!(launch.args, ["-d", "Ubuntu 24.04"]);
    }

    /// Asking WSL to pick its own default is not the same as this program picking one,
    /// and the difference shows the moment the user changes their default: no `-d` means
    /// WSL answers the question every time it is asked.
    #[test]
    fn the_default_distribution_is_left_to_wsl_rather_than_named() {
        let launch = Wsl::new("wsl.exe", Some("bash")).launch();

        assert!(
            launch.args.is_empty(),
            "no distribution is invented for a session that did not name one"
        );
    }

    /// The launch carries the client as the user named it, for the reason cmd's does: a
    /// full path is a legitimate way to name the same program.
    #[test]
    fn the_launch_carries_the_client_it_was_named_by() {
        let launch =
            Wsl::in_distribution(r"C:\Windows\system32\wsl.exe", "Debian", Some("bash")).launch();

        assert_eq!(launch.program, r"C:\Windows\system32\wsl.exe");
    }

    /// **Nothing is armed at launch, whatever the distribution runs** (spec B9.5,
    /// decision 1). The `PROMPT_COMMAND` and the `WSLENV` entry that carried it are gone, and
    /// this is the assertion that stops either coming back: they existed to get a program in
    /// before `.bashrc` ran, and 23.11 measured that going first is what an ordinary dotfile
    /// beats.
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

    /// The launch does not depend on which distribution it lands in, and since B9.5 it does
    /// not depend on the shell either — which is what lets the client be started while the
    /// probe is still being answered.
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

    /// **The setup is the shell's, and it is the same one SSH gets for the same name** (spec
    /// B9.5, decision 2) — the first time those two transports share a strategy rather than
    /// having one each.
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

    /// **What `sh`'s own line earns, and it is not bash's claim** (spec B9.5, decision 8, as
    /// roadmap 23.15 revised it). `sh` marks its prompt and reports a verdict and still says
    /// nothing about where output begins, and the session is told that before its first byte
    /// rather than discovering it from the absence of a marker.
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

    /// **The case B5.5 exists for, unchanged by the new mechanism.** An account running zsh
    /// is named and nothing is run in it — and the session still claims the full cycle, so
    /// the grace period is what tells the listener the truth.
    #[test]
    fn a_distribution_running_a_shell_nobody_measured_has_nothing_run_in_it() {
        for named in ["zsh", "fish", "nu"] {
            let adapter = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some(named));

            assert_eq!(adapter.setup(), None, "{named} has no measured setup");
            assert_eq!(
                adapter.markers(),
                ShellMarkers::Full,
                "{named} is claimed optimistically, so the grace period can contradict it"
            );
        }
    }

    /// A distribution that would not answer is in the same position as one running zsh: the
    /// session starts and nothing is claimed about it.
    #[test]
    fn a_distribution_that_answered_nothing_has_nothing_run_in_it() {
        let adapter = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", None);

        assert_eq!(adapter.setup(), None);
        assert_eq!(adapter.markers(), ShellMarkers::Full);
    }

    /// The name is carried through for the sentence a listener hears, whether or not it is
    /// a shell Acter has a setup for.
    #[test]
    fn the_shell_the_distribution_named_is_what_the_session_is_called_with() {
        assert_eq!(
            Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("zsh")).login_shell(),
            Some("zsh")
        );
        assert_eq!(Wsl::new("wsl.exe", None).login_shell(), None);
    }

    /// The probe's answer can be applied after the client has been started, which is what
    /// lets the two happen side by side (spec B9.5, decision 14).
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

    /// **The transport is part of the measurement, and this is where that is asserted.**
    /// `far_end::over_ssh` answers `0x04` for bash because B9 sent that byte down a real
    /// SSH connection and watched the session close. Nobody has done it here, so bash under
    /// WSL still ends on nothing — a different cell of the matrix and a different
    /// measurement, not an inconsistency to tidy away.
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
