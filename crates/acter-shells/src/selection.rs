//! Policy: which adapter a program name selects.
//!
//! The knowledge is one match, and it is the only place in the system that turns a name a
//! user typed into a shell Acter has something to say about. It lives here rather than in
//! the composition root because "`cmd.exe` is cmd" is shell knowledge, and rather than in
//! `lib.rs` because a facade declares modules and re-exports, and this has behaviour and
//! tests of its own.
//!
//! **Finding out which shells a machine has is not this** — that is I/O, it lives behind
//! `ThisComputer`, and it arrived in B5.3 with the adapter that needs it. A program name
//! is all this function has, which is why `wsl.exe` selects a session in whatever
//! distribution WSL calls the default: naming one is the connect list's business, and the
//! connect list builds that adapter directly rather than through here.
//!
//! **Since B5.5 that limit costs the WSL client its setup here, and it should.** What
//! `wsl.exe` starts is whatever login shell the distribution's account runs, which is another
//! thing only I/O can find out — so a name alone no longer licenses running bash's setup in
//! it. This selects a WSL session with nothing claimed about its far end; a caller that has
//! *asked* builds [`Wsl`] directly with the answer, which is what the composition root does
//! for every WSL start including `wsl.exe` named on its own.
//!
//! **Since B9.5 no launch carries a setup at all**, so what an unasked client loses is no
//! longer part of its launch: nothing is armed when the client is started, and what makes a
//! far end mark its boundaries is a line sent into the session once it is up. What a name
//! alone still cannot license is that line.

use acter_core::ShellAdapter;

use crate::cmd::{self, Cmd};
use crate::plain::Plain;
use crate::powershell::{self, PowerShell};
use crate::wsl::{self, Wsl};

/// The adapter for the shell this program names, and [`Plain`](crate::Plain) for one this
/// crate does not recognise.
///
/// Never fails and never falls back silently to a *different* shell: an unknown program is
/// still started, just with nothing injected into it.
pub fn adapter_for(program: &str) -> Box<dyn ShellAdapter> {
    if cmd::is_cmd(program) {
        Box::new(Cmd::new(program))
    } else if powershell::is_powershell(program) {
        Box::new(PowerShell::new(program))
    } else if wsl::is_wsl(program) {
        // `None`, because a name is all there is here and the far end has not been asked
        // (spec B5.5, decision 4). An unasked distribution is started as it stands, which
        // is the third of decision 4's three states rather than a degradation.
        Box::new(Wsl::new(program, None))
    } else {
        Box::new(Plain::new(program))
    }
}

#[cfg(test)]
mod tests {
    use acter_core::{ShellFacts, ShellMarkers};

    use super::*;

    /// The three ways a user can name cmd all reach cmd's adapter. Asserted through the
    /// port rather than by downcasting: what a caller can observe is the launch and the
    /// markers, and those are what must be right.
    #[test]
    fn cmd_is_selected_however_it_was_named() {
        for named in ["cmd", "cmd.exe", "CMD.EXE", r"C:\Windows\system32\cmd.exe"] {
            let adapter = adapter_for(named);

            assert_eq!(
                adapter.markers(),
                ShellMarkers::PromptAndCommandLine,
                "{named} is cmd"
            );
            assert_eq!(adapter.launch().program, named, "started as it was named");
            assert!(
                !adapter.launch().environment.is_empty(),
                "{named} gets the prompt injection"
            );
        }
    }

    /// `wsl.exe` reaches the WSL adapter, in whatever distribution WSL calls the default.
    /// Asserted through the port for the reason cmd's is: what a caller can observe is the
    /// launch and the markers.
    ///
    /// **It gets no setup here, and that changed with B5.5.** A name does not say which shell
    /// the distribution's account runs, so nothing licenses running bash's line in it from
    /// this function. The composition root asks and then builds [`Wsl`] with the answer; what
    /// is asserted here is that a name alone selects the right adapter and claims nothing
    /// about its far end.
    #[test]
    fn the_wsl_client_is_selected_however_it_was_named() {
        for named in ["wsl", "wsl.exe", "WSL.EXE", r"C:\Windows\system32\wsl.exe"] {
            let adapter = adapter_for(named);

            assert_eq!(
                adapter.markers(),
                ShellMarkers::Full,
                "{named} marks the whole cycle"
            );
            assert_eq!(adapter.launch().program, named, "started as it was named");
            assert!(
                adapter.launch().environment.is_empty(),
                "{named} starts with an empty environment, as every shell does since B9.5"
            );
            assert!(
                adapter.launch().args.is_empty(),
                "{named} names no distribution, so WSL picks its own default"
            );
        }
    }

    /// **The distinction the test above no longer draws, drawn here instead.** An
    /// unrecognised program and an unasked WSL client produce the same launch, so what
    /// separates them is what happens once somebody asks: only the WSL client can be told what
    /// its far end runs and be given a setup to run inside it.
    ///
    /// **The difference moved out of the launch with B9.5** (decision 1). It used to be two
    /// environment variables — a `PROMPT_COMMAND` and the `WSLENV` entry that carried it
    /// across the kernel boundary — and it is now a line sent into the session after it is
    /// established, which is the only ordering in which the user's own startup files do not
    /// get the last word.
    #[test]
    fn a_wsl_client_that_was_asked_is_the_one_that_gets_a_setup() {
        assert!(
            Wsl::new("wsl.exe", Some("bash")).setup().is_some(),
            "a distribution known to run bash has a line to run inside it"
        );
        assert_eq!(
            adapter_for("wsl.exe").setup(),
            None,
            "and one nobody asked has nothing run in it at all"
        );
        assert_eq!(
            adapter_for("wsl.exe").launch().environment,
            Wsl::new("wsl.exe", Some("bash")).launch().environment,
            "which is a difference in what is run, not in how the client is started"
        );
    }

    /// Both editions reach PowerShell's adapter, and each is started by the name it was
    /// asked for: which edition a user gets is decided by which executable they named, so
    /// resolving `pwsh.exe` to something that starts `powershell.exe` would hand them the
    /// other PowerShell without saying so.
    #[test]
    fn either_powershell_edition_is_selected_however_it_was_named() {
        for named in [
            "powershell",
            "powershell.exe",
            "pwsh",
            "PWSH.EXE",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
        ] {
            let adapter = adapter_for(named);

            assert_eq!(adapter.launch().program, named, "started as it was named");
            assert!(
                adapter.launch().args.contains(&"-Command".to_owned()),
                "{named} gets the snippet injection"
            );
            assert!(
                adapter.eof().is_some(),
                "{named} knows what ends one of its sessions"
            );
        }
    }

    /// The null adapter is what is left over — and **`Full` markers alone stopped
    /// identifying it**, because two real shells now claim them as well. So this asserts
    /// on the whole launch: what separates an unrecognised shell from PowerShell or from
    /// WSL is that it is started exactly as it stands, with nothing injected into it and
    /// no end-of-input anyone measured.
    ///
    /// `wslconfig.exe` is in the list deliberately: it begins with the four letters that
    /// name the WSL client and is a different program, so a prefix match would claim it.
    #[test]
    fn a_shell_this_crate_does_not_know_gets_the_null_adapter() {
        for named in ["bash", "nushell.exe", r"C:\bin\wslconfig.exe"] {
            let adapter = adapter_for(named);

            assert_eq!(adapter.markers(), ShellMarkers::Full, "{named} is unknown");
            assert_eq!(adapter.launch(), Plain::new(named).launch());
            assert_eq!(
                adapter.eof(),
                None,
                "{named} has no end-of-input answer anyone measured"
            );
        }
    }

    /// **What the session is handed is the adapter's own answers, unedited.** `ShellFacts`
    /// is the value `SessionService::start` takes, and the composition root builds it from
    /// whichever adapter `adapter_for` returned — so a shell whose markers or end-of-input
    /// were dropped or swapped on the way through would produce a session that behaves like
    /// a different shell, which is the failure B4.5 measured and the reason the two travel
    /// together at all.
    #[test]
    fn the_facts_handed_to_a_session_are_the_adapters_own() {
        for named in ["cmd.exe", "powershell.exe", "wsl.exe", "nushell.exe"] {
            let adapter = adapter_for(named);
            let facts = ShellFacts::of(adapter.as_ref());

            assert_eq!(facts.markers, adapter.markers(), "{named}'s markers");
            assert_eq!(facts.eof, adapter.eof(), "{named}'s end-of-input");
        }
    }

    /// The same, spelled out for the two shells that disagree about it, so that a change to
    /// either is a change to this file too: PowerShell knows what ends one of its sessions
    /// and cmd does not.
    #[test]
    fn a_shell_with_a_measured_ending_is_told_apart_from_one_without() {
        assert!(
            ShellFacts::of(adapter_for("powershell.exe").as_ref())
                .eof
                .is_some(),
            "PowerShell was measured, so a session over it can be ended"
        );
        assert_eq!(
            ShellFacts::of(adapter_for("cmd.exe").as_ref()).eof,
            None,
            "cmd was not, so Acter says so rather than guessing at a byte"
        );
    }
}
