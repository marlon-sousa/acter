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
//! Where the marker program came from, and every byte of what it emits, is in
//! [`injection`]; how a distribution list is read out of `wsl.exe -l -q` is in
//! [`distributions`].

pub(crate) mod distributions;
mod injection;

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

use crate::wsl::injection::ENVIRONMENT;

/// The `-d` flag that points `wsl.exe` at one distribution rather than at the default.
const DISTRIBUTION_FLAG: &str = "-d";

/// A bash session inside one WSL distribution.
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
}

impl Wsl {
    /// A session in whatever distribution WSL considers the default — `wsl.exe` with no
    /// `-d` at all.
    ///
    /// Not a guess at which one that is: asking WSL to choose is a different thing from
    /// this program choosing, and only the first of the two is ever right after the user
    /// changes their default. Measured on 2026-08-24 as starting the same integrated bash
    /// as a named distribution does.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            distribution: None,
        }
    }

    /// A session in the distribution with this name, as `wsl.exe -l -q` spelled it.
    pub fn in_distribution(program: impl Into<String>, distribution: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            distribution: Some(distribution.into()),
        }
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
            environment: ENVIRONMENT
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
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
    fn markers(&self) -> ShellMarkers {
        ShellMarkers::Full
    }
}

/// Whether this program is the WSL client, however it was named.
///
/// Case-insensitive and extension-insensitive for [`is_cmd`](crate::cmd) reasons: `wsl`,
/// `wsl.exe` and a full path to it are all the same client and a user typing any of them
/// means the same thing.
pub(crate) fn is_wsl(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    program.eq_ignore_ascii_case("wsl") || program.eq_ignore_ascii_case("wsl.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let launch = Wsl::in_distribution("wsl.exe", "Ubuntu 24.04").launch();

        assert_eq!(launch.program, "wsl.exe");
        assert_eq!(launch.args, ["-d", "Ubuntu 24.04"]);
    }

    /// Asking WSL to pick its own default is not the same as this program picking one,
    /// and the difference shows the moment the user changes their default: no `-d` means
    /// WSL answers the question every time it is asked.
    #[test]
    fn the_default_distribution_is_left_to_wsl_rather_than_named() {
        let launch = Wsl::new("wsl.exe").launch();

        assert!(
            launch.args.is_empty(),
            "no distribution is invented for a session that did not name one"
        );
    }

    /// The launch carries the client as the user named it, for the reason cmd's does: a
    /// full path is a legitimate way to name the same program.
    #[test]
    fn the_launch_carries_the_client_it_was_named_by() {
        let launch = Wsl::in_distribution(r"C:\Windows\system32\wsl.exe", "Debian").launch();

        assert_eq!(launch.program, r"C:\Windows\system32\wsl.exe");
    }

    /// Both halves of the injection, in one place, because a `PROMPT_COMMAND` that does
    /// not cross the kernel boundary is a launch that looks integrated and marks nothing.
    #[test]
    fn every_session_is_started_with_the_injection_and_with_wslenv_carrying_it() {
        let launch = Wsl::new("wsl.exe").launch();
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
            Wsl::new("wsl.exe").launch().environment,
            Wsl::in_distribution("wsl.exe", "Debian")
                .launch()
                .environment
        );
    }

    /// The claim the whole entry turns on, asserted where a reader will look for it.
    #[test]
    fn a_wsl_session_claims_the_full_marker_cycle() {
        assert_eq!(Wsl::new("wsl.exe").markers(), ShellMarkers::Full);
    }
}
