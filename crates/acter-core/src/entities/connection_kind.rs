//! Entity/value: the kinds of far end Acter can connect to, what each is called, and what
//! it says when this machine cannot start it.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum ConnectionKind {
    Cmd,
    /// One row; its editions are the two below.
    PowerShell,
    /// 5.1, shipped with Windows.
    WindowsPowerShell,
    /// 7 or later.
    PowerShellSeven,
    Wsl,
    /// A macOS login shell, with `/etc/shells` as its variants.
    Terminal,
    Ssh,
}

impl ConnectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cmd => "Command Prompt",
            Self::PowerShell => "PowerShell",
            Self::WindowsPowerShell => "Windows PowerShell",
            Self::PowerShellSeven => "PowerShell 7",
            Self::Wsl => "WSL",
            Self::Terminal => "Terminal",
            Self::Ssh => "SSH",
        }
    }

    /// A file name that [`ThisComputer::installs`](crate::ThisComputer::installs) resolves;
    /// empty for a kind with no local executable.
    pub fn program(self) -> &'static str {
        match self {
            Self::Cmd => "cmd.exe",
            Self::PowerShell => "powershell.exe",
            Self::WindowsPowerShell => "powershell.exe",
            Self::PowerShellSeven => "pwsh.exe",
            Self::Wsl => "wsl.exe",
            // `login_shells` answers instead.
            Self::Terminal => "",
            Self::Ssh => "",
        }
    }

    /// Empty for every kind but PowerShell.
    pub fn editions(self) -> &'static [ConnectionKind] {
        match self {
            Self::PowerShell => &[Self::WindowsPowerShell, Self::PowerShellSeven],
            _ => &[],
        }
    }

    /// A command to type, never a GUI route described in prose.
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Cmd => {
                "Command Prompt is missing from this Windows installation. It is part of \
                 Windows itself, so this usually means the system files are damaged. Run \
                 sfc /scannow from an administrator Command Prompt to check them."
            }
            Self::PowerShell => {
                "No edition of PowerShell is installed on this computer. Windows PowerShell                  ships with Windows, so this usually means it was removed as an optional                  feature. Reinstall it from Settings, under System, Optional features."
            }
            Self::WindowsPowerShell => {
                "Windows PowerShell is missing from this Windows installation. It ships \
                 with Windows, so this usually means it was removed as an optional \
                 feature. Reinstall it from Settings, under System, Optional features."
            }
            Self::PowerShellSeven => {
                "PowerShell 7 is not installed. It is a separate product from the Windows \
                 PowerShell that ships with Windows. Install it by running winget install \
                 Microsoft.PowerShell from any terminal."
            }
            Self::Wsl => {
                "No WSL distribution is installed. WSL runs Linux inside Windows, and needs \
                 a distribution before there is anything to connect to. Install one by \
                 running wsl --install from an administrator Command Prompt, then restart \
                 the computer when it asks."
            }
            Self::Terminal => {
                "This Mac lists no shells an account can log in to. That list is the file \
                 /etc/shells, and a macOS install always has one, so this usually means the \
                 file has been emptied or replaced. Run cat /etc/shells in any terminal to \
                 see what it holds."
            }
            Self::Ssh => {
                "SSH is built into Acter, so it cannot be missing. If a connection will not \
                 start, the reason is with the server or the network rather than with this \
                 computer."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_KIND: [ConnectionKind; 7] = [
        ConnectionKind::Cmd,
        ConnectionKind::PowerShell,
        ConnectionKind::WindowsPowerShell,
        ConnectionKind::PowerShellSeven,
        ConnectionKind::Wsl,
        ConnectionKind::Terminal,
        ConnectionKind::Ssh,
    ];

    #[test]
    fn every_kind_is_named() {
        for kind in EVERY_KIND {
            assert!(!kind.label().trim().is_empty(), "{kind:?} has a label");
        }
    }

    #[test]
    fn no_two_kinds_are_called_the_same_thing() {
        let mut seen = Vec::new();
        for kind in EVERY_KIND {
            assert!(
                !seen.contains(&kind.label()),
                "{:?} is called the same as something else",
                kind
            );
            seen.push(kind.label());
        }
    }

    #[test]
    fn every_kind_says_what_to_do_about_being_missing() {
        for kind in EVERY_KIND {
            let said = kind.instructions();

            assert!(!said.trim().is_empty(), "{kind:?} has instructions");
            assert!(
                said.ends_with('.'),
                "{kind:?} speaks in sentences, and a sentence ends: {said:?}"
            );
        }
    }

    #[test]
    fn the_instructions_name_a_command_that_can_be_typed() {
        for (kind, command) in [
            (ConnectionKind::Cmd, "sfc /scannow"),
            (ConnectionKind::PowerShellSeven, "winget install"),
            (ConnectionKind::Wsl, "wsl --install"),
        ] {
            assert!(
                kind.instructions().contains(command),
                "{kind:?} tells the user to run {command}"
            );
        }
    }

    #[test]
    fn every_kind_names_an_executable_except_the_one_that_is_not_a_program() {
        for kind in EVERY_KIND {
            let program = kind.program();
            if matches!(kind, ConnectionKind::Ssh | ConnectionKind::Terminal) {
                assert!(
                    program.is_empty(),
                    "{kind:?} names no program compiled into Acter"
                );
                continue;
            }
            assert!(
                program.ends_with(".exe"),
                "{kind:?} names an executable: {program}"
            );
        }
    }

    #[test]
    fn the_editions_of_a_kind_name_different_programs() {
        let editions = ConnectionKind::PowerShell.editions();

        assert_eq!(editions.len(), 2);
        assert_ne!(editions[0].program(), editions[1].program());
    }

    #[test]
    fn a_kind_with_editions_names_one_of_them() {
        let kind = ConnectionKind::PowerShell;

        assert!(
            kind.editions()
                .iter()
                .any(|edition| edition.program() == kind.program()),
            "PowerShell names an edition rather than an executable of its own"
        );
        assert_eq!(kind.program(), ConnectionKind::WindowsPowerShell.program());
    }

    #[test]
    fn a_kind_that_is_one_thing_has_no_editions() {
        for kind in [
            ConnectionKind::Cmd,
            ConnectionKind::Wsl,
            ConnectionKind::Terminal,
        ] {
            assert!(kind.editions().is_empty(), "{kind:?} is one thing");
        }
    }

    #[test]
    fn the_one_kind_with_no_command_names_a_place_instead() {
        let said = ConnectionKind::WindowsPowerShell.instructions();

        assert!(said.contains("Settings"), "it names where to go: {said:?}");
        assert!(
            said.contains("Optional features"),
            "and how far in: {said:?}"
        );
    }
}
