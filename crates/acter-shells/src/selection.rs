//! Policy: which adapter a program name selects.

use acter_core::ShellAdapter;

use crate::cmd::{self, Cmd};
use crate::plain::Plain;
use crate::powershell::{self, PowerShell};
use crate::wsl::{self, Wsl};

/// [`Plain`](crate::Plain) for a program this crate does not recognise.
pub fn adapter_for(program: &str) -> Box<dyn ShellAdapter> {
    if cmd::is_cmd(program) {
        Box::new(Cmd::new(program))
    } else if powershell::is_powershell(program) {
        Box::new(PowerShell::new(program))
    } else if wsl::is_wsl(program) {
        // `None`: the far end has not been asked what shell it runs.
        Box::new(Wsl::new(program, None))
    } else {
        Box::new(Plain::new(program))
    }
}

#[cfg(test)]
mod tests {
    use acter_core::{ShellFacts, ShellMarkers};

    use super::*;

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

    #[test]
    fn the_facts_handed_to_a_session_are_the_adapters_own() {
        for named in ["cmd.exe", "powershell.exe", "wsl.exe", "nushell.exe"] {
            let adapter = adapter_for(named);
            let facts = ShellFacts::of(adapter.as_ref());

            assert_eq!(facts.markers, adapter.markers(), "{named}'s markers");
            assert_eq!(facts.eof, adapter.eof(), "{named}'s end-of-input");
        }
    }

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
