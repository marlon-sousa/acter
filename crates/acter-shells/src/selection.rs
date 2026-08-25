//! Policy: which adapter a program name selects.
//!
//! The knowledge is one match, and it is the only place in the system that turns a name a
//! user typed into a shell Acter has something to say about. It lives here rather than in
//! the composition root because "`cmd.exe` is cmd" is shell knowledge, and rather than in
//! `lib.rs` because a facade declares modules and re-exports, and this has behaviour and
//! tests of its own.
//!
//! **Finding out which shells a machine has is not this** — that is I/O, it lives behind
//! `InstalledShells`, and it arrived in B5.3 with the adapter that needs it. A program name
//! is all this function has, which is why `wsl.exe` selects a session in whatever
//! distribution WSL calls the default: naming one is the connect list's business, and the
//! connect list builds that adapter directly rather than through here.

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
        Box::new(Wsl::new(program))
    } else {
        Box::new(Plain::new(program))
    }
}

#[cfg(test)]
mod tests {
    use acter_core::ShellMarkers;

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

    /// `wsl.exe` reaches the WSL adapter and gets the injection, in whatever distribution
    /// WSL calls the default. Asserted through the port for the reason cmd's is: what a
    /// caller can observe is the launch and the markers.
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
            assert_eq!(
                adapter
                    .launch()
                    .environment
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<String>>(),
                ["WSLENV", "PROMPT_COMMAND"],
                "{named} gets the injection and the entry that carries it across"
            );
            assert!(
                adapter.launch().args.is_empty(),
                "{named} names no distribution, so WSL picks its own default"
            );
        }
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
}
