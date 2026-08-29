//! Adapter: bash inside a WSL distribution, reached through `wsl.exe`, behind the
//! `ShellAdapter` port — how the client is started, which distribution it is pointed at,
//! and the `PROMPT_COMMAND` program that makes bash mark its own command boundaries.
//!
//! **The first far end Acter sets up that is not in its own process tree.** `wsl.exe` is a
//! client to a shell in another kernel, exactly as `docker.exe` is (ROADMAP 22.6), and two
//! things follow that no earlier adapter had to think about. An environment variable set
//! on the Windows side is *not* in the distribution's environment unless `WSLENV` names it,
//! which is why the injection here is two variables rather than one. And an interrupt does
//! cross, because it travels as data: `0x03` written to the pseudoconsole reaches bash's
//! own line discipline, which turns it into `SIGINT` — measured by 22.6, and re-measured
//! here on 2026-08-24 as the exit code `130` arriving in a `D` marker.
//!
//! **The one adapter whose variants are discovered rather than known.** `Cmd` is the same
//! shell on every Windows machine; which distributions exist is the same answer on no two
//! machines at all, so *that* question is I/O and lives behind
//! [`InstalledShells`](acter_core::InstalledShells) rather than here. This module knows
//! only how to start one once its name is in hand.
//!
//! **Since B5.5, which shell it starts is discovered too.** `wsl.exe` is a client, and what
//! it runs is whatever login shell the distribution's account carries in its own passwd
//! entry — so this adapter is *told* what that shell is rather than assuming bash, and it
//! injects only when it was told the one shell the injection was measured against. An
//! account running zsh is named and left alone. The same port answers that question, on the
//! same client, and [`login_shell`] is what its answer means. What it is *not* is a gate: a
//! distribution that would not answer starts anyway, with nothing claimed about it.
//!
//! Where the marker program came from, and every byte of what it emits, is in
//! [`injection`]; how a distribution list is read out of `wsl.exe -l -q` is in
//! [`distributions`].

pub(crate) mod distributions;
mod injection;
pub(crate) mod login_shell;

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

use crate::wsl::injection::{ENVIRONMENT, MEASURED};

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
    /// **Carried rather than assumed, since B5.5.** Every constructor here takes it, so
    /// there is no way to build this adapter without having decided what to believe about
    /// the far end. Before that the field did not exist and bash was the silent answer
    /// everywhere, including in the accounts that do not run it.
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

    /// What this session is with, for the sentence a listener hears, and `None` when the
    /// distribution said nothing this program is willing to say out loud.
    pub fn login_shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }

    /// Whether Acter put its marker program into this session.
    ///
    /// **True for exactly one shell** (spec B5.5, decision 4), which is the one the
    /// injection was measured against. It is *not* the same question as
    /// [`markers`](ShellAdapter::markers): every WSL session claims the full cycle, and the
    /// grace period is what contradicts that claim for a session nothing was injected into.
    /// This is what the composition root asks in order to decide whether the connection
    /// sentence carries "with no shell integration set up in this distribution".
    pub fn is_integrated(&self) -> bool {
        self.shell.as_deref() == Some(MEASURED)
    }
}

impl ShellAdapter for Wsl {
    fn launch(&self) -> ShellLaunch {
        let mut args = Vec::new();
        if let Some(distribution) = &self.distribution {
            args.push(DISTRIBUTION_FLAG.to_owned());
            args.push(distribution.clone());
        }
        // **Nothing is pushed into a shell there is no reason to believe reads it** (spec
        // B5.5, decision 4). `PROMPT_COMMAND` is bash's, and an account running zsh or fish
        // would carry it across the kernel boundary in `WSLENV` for nothing to read at the
        // other end. A distribution that did not answer is in the same position: the session
        // starts, and nothing is claimed about it.
        let environment = match self.is_integrated() {
            true => ENVIRONMENT
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            false => Vec::new(),
        };
        ShellLaunch {
            program: self.program.clone(),
            args,
            environment,
        }
    }

    /// `Full`, and the first adapter for which that is a *measurement* rather than the
    /// assumption [`Plain`](crate::Plain) makes.
    ///
    /// cmd's prompt can carry `A` and `B` and has no post-execution hook, so its adapter
    /// declares less than the full cycle and the tracker synthesizes what is missing.
    /// Bash has `PROMPT_COMMAND` and a `DEBUG` trap, so all four markers arrive and `D`
    /// carries a real exit code — which makes a WSL session the first shipped one where a
    /// listener is told how a command went rather than only that the prompt came back.
    ///
    /// **Still `Full` for a distribution nothing was injected into, and that is deliberate**
    /// (spec B5.5, decision 4, following B9's decision 2). `Full` is an optimistic claim
    /// rather than a report, and it is the claim the startup grace period exists to
    /// contradict: a session that never marks anything reaches `IntegrationUnavailable` and
    /// says so. Answering `PromptAndCommandLine` here for zsh would forge a measurement
    /// nobody made, and answering with something narrower would leave a listener waiting for
    /// boundaries in silence.
    fn markers(&self) -> ShellMarkers {
        ShellMarkers::Full
    }

    /// **Nobody has measured what ends a bash session through this transport, so this says
    /// so rather than guessing.**
    ///
    /// `0x04` is the obvious answer and it is probably right: bash reads from a
    /// pseudoconsole whose line discipline turns that byte into end-of-file. But "probably
    /// right" is exactly what B5.2 measured and disproved for the shell next door —
    /// *neither* control byte that is supposed to end a PowerShell session does, both are
    /// echoed as caret text, and a line submitted behind one runs as a command the user
    /// never typed. A byte assumed here would fail in the same shape: silently, in front of
    /// a user who pressed Ctrl+D and cannot see what happened.
    ///
    /// `None` means "Acter does not know how to end this shell", which the session reports
    /// out loud. It becomes a byte the day somebody drives a real distribution and watches
    /// the session close.
    fn eof(&self) -> Option<Vec<u8>> {
        None
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

    /// Both halves of the injection, in one place, because a `PROMPT_COMMAND` that does
    /// not cross the kernel boundary is a launch that looks integrated and marks nothing.
    #[test]
    fn every_session_is_started_with_the_injection_and_with_wslenv_carrying_it() {
        let launch = Wsl::new("wsl.exe", Some("bash")).launch();
        let names: Vec<&str> = launch
            .environment
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();

        assert_eq!(names, ["WSLENV", "PROMPT_COMMAND"]);
        assert_eq!(
            launch
                .environment
                .iter()
                .find(|(name, _)| name == "WSLENV")
                .map(|(_, value)| value.as_str()),
            Some("PROMPT_COMMAND"),
            "WSLENV names the variable that has to cross, and nothing else"
        );
    }

    /// The injection does not depend on which distribution it lands in — measured against
    /// Ubuntu 24.04 and Debian on 2026-08-24, which produced the same marker stream — so
    /// the environment is the same whichever one is named.
    #[test]
    fn the_injection_does_not_change_with_the_distribution() {
        assert_eq!(
            Wsl::new("wsl.exe", Some("bash")).launch().environment,
            Wsl::in_distribution("wsl.exe", "Debian", Some("bash"))
                .launch()
                .environment
        );
    }

    /// The claim the whole entry turns on, asserted where a reader will look for it.
    #[test]
    fn a_wsl_session_claims_the_full_marker_cycle() {
        assert_eq!(
            Wsl::new("wsl.exe", Some("bash")).markers(),
            ShellMarkers::Full
        );
    }

    /// **The case the whole entry exists for.** An account running zsh gets no
    /// `PROMPT_COMMAND` and no `WSLENV`: pushing bash's program at it would carry a
    /// variable across the kernel boundary for nothing to read at the other end.
    #[test]
    fn a_distribution_that_does_not_run_bash_is_started_with_nothing_injected() {
        for named in ["zsh", "fish", "dash", "sh", "nu"] {
            let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some(named)).launch();

            assert!(
                launch.environment.is_empty(),
                "{named} is not the shell the injection was measured against"
            );
            assert_eq!(
                launch.args,
                ["-d", "Ubuntu 24.04"],
                "{named} still starts, in the distribution that was named"
            );
        }
    }

    /// A distribution that would not answer is in the same position as one running zsh:
    /// the session starts and nothing is claimed about it. The probe is advisory, so its
    /// silence costs a session nothing except the injection it could not justify.
    #[test]
    fn a_distribution_that_answered_nothing_is_started_with_nothing_injected() {
        let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", None).launch();

        assert!(launch.environment.is_empty());
        assert_eq!(launch.args, ["-d", "Ubuntu 24.04"]);
    }

    /// What the composition root asks in order to decide whether the connection sentence
    /// carries the missing-integration clause — and it is true for exactly one shell.
    #[test]
    fn only_the_measured_shell_counts_as_integrated() {
        assert!(Wsl::new("wsl.exe", Some("bash")).is_integrated());
        for named in ["zsh", "fish", "dash", "sh", "Bash", "bash5"] {
            assert!(
                !Wsl::new("wsl.exe", Some(named)).is_integrated(),
                "{named} has not been measured under WSL"
            );
        }
        assert!(!Wsl::new("wsl.exe", None).is_integrated());
    }

    /// The name is carried through for the sentence a listener hears, whether or not it is
    /// a shell Acter integrated.
    #[test]
    fn the_shell_the_distribution_named_is_what_the_session_is_called_with() {
        assert_eq!(
            Wsl::in_distribution("wsl.exe", "Ubuntu 24.04", Some("zsh")).login_shell(),
            Some("zsh")
        );
        assert_eq!(Wsl::new("wsl.exe", None).login_shell(), None);
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
