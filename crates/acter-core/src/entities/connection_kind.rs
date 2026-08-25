//! Entity/value: the kinds of far end Acter can connect to, what each is called, and what
//! it says when this machine cannot start it.
//!
//! **A kind is not an adapter.** `acter-shells` knows how to *start* cmd; this knows that
//! cmd is a thing a user can choose, what it is called in a list, and what to tell somebody
//! whose machine does not have it. The two are separate because the same adapter appears in
//! the list more than once — two PowerShell editions, one entry per WSL distribution — and
//! because a kind has to exist in order to be reported missing, which an adapter for a shell
//! nobody installed cannot usefully do (spec B5.4, decision 6).
//!
//! **Every string here is a domain requirement.** They are read aloud, so they are written
//! to be heard: what is missing first, then what to do, then where. A listener hears the
//! first clause before deciding whether the rest is worth their attention (CLAUDE.md).

/// One thing a user can choose to connect to.
///
/// Deliberately not carrying the adapter that starts it: this is what the connect list is
/// made of, and the list exists on machines where the thing cannot be started at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionKind {
    /// `cmd.exe`, which is on every Windows machine and cannot be removed.
    Cmd,
    /// Windows PowerShell 5.1, which ships with Windows.
    WindowsPowerShell,
    /// PowerShell 7 or later, which is installed separately.
    PowerShellSeven,
    /// Bash inside a WSL distribution.
    Wsl,
}

impl ConnectionKind {
    /// What this kind is called in the list, with nothing about whether it can be started.
    ///
    /// The two editions are named the way Microsoft names them rather than by their
    /// executables: somebody choosing between them is choosing between products they have
    /// read about, not between `powershell.exe` and `pwsh.exe`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cmd => "Command Prompt",
            Self::WindowsPowerShell => "Windows PowerShell",
            Self::PowerShellSeven => "PowerShell 7",
            Self::Wsl => "WSL",
        }
    }

    /// What to tell somebody whose machine cannot start this kind: what is missing, what to
    /// type, and where.
    ///
    /// **A command rather than a path through a graphical installer.** A GUI route a blind
    /// user has to follow by description is worse than a line they can type or paste, and
    /// every one of these is a line they can type (spec B5.4, decision 4).
    ///
    /// The two kinds that cannot go missing still answer, because "this is impossible" is a
    /// worse thing for a program to assume than for it to have a sentence it never says:
    /// a Windows install with no `cmd.exe` is broken rather than unsupported, and saying so
    /// is more use than a panic or an empty box.
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Cmd => {
                "Command Prompt is missing from this Windows installation. It is part of \
                 Windows itself, so this usually means the system files are damaged. Run \
                 sfc /scannow from an administrator Command Prompt to check them."
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_KIND: [ConnectionKind; 4] = [
        ConnectionKind::Cmd,
        ConnectionKind::WindowsPowerShell,
        ConnectionKind::PowerShellSeven,
        ConnectionKind::Wsl,
    ];

    #[test]
    fn every_kind_is_named() {
        for kind in EVERY_KIND {
            assert!(!kind.label().trim().is_empty(), "{kind:?} has a label");
        }
    }

    /// The labels have to differ, because the list is navigated by ear and two rows that
    /// sound identical cannot be told apart. The two PowerShell editions are the pair this
    /// is really about.
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

    /// **Every kind answers, including the two that should never go missing.** A program
    /// that assumes an absence is impossible has nothing to say on the day it happens.
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

    /// The one thing a listener is being asked to *do* has to be in the text, spelled the
    /// way they would type it. This is the assertion that catches instructions rewritten
    /// into prose that sounds helpful and tells nobody anything.
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

    /// Windows PowerShell is the exception and it is deliberate: there is no command that
    /// reinstalls an optional feature reliably across Windows editions, so it names the
    /// place instead — and names it in the order the user will walk it.
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
