//! Entity/value: one install of a shell that this machine actually has — the file itself,
//! where it came from, and what `PATH` says about it.
//!
//! **The answer to "is this installed" stopped being a boolean with B5.7.** It used to be:
//! the machine was asked whether a *name* could be started, and the transport later started
//! that same name, so Windows resolved it a second time and nothing guaranteed the two
//! resolutions landed on the same file. Checking a signature under that regime would be
//! theatre — verify one file, start whichever file `PATH` happened to name a moment later.
//! So availability answers with the path it found, that path is what is verified, and that
//! path is what is started (spec B5.7, decision 1).
//!
//! **What identifies an install is where it came from, never what the file says about
//! itself** (decision 3). Measured 2026-08-27: `powershell.exe` reports FileVersion
//! `10.0.26100.8875` — the Windows build, not 5.1 — so a design that read the version
//! resource would be wrong for one of the two editions in exactly the direction that
//! matters. Windows Terminal reached the same conclusion independently: it takes the
//! version from the directory name or the package identity and never opens the file.

use std::path::{Path, PathBuf};

/// One file this machine can start, found once and started as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInstall {
    /// The file itself, fully resolved. **This is what gets verified and what gets
    /// started**, which is the whole of decision 1: the window between the check and the
    /// spawn does not close completely — nothing on Windows closes it — but it narrows from
    /// "any file on `PATH`" to "this file, moved or replaced in the seconds since".
    pub program: PathBuf,
    /// Where it came from, which is what tells two installs of the same edition apart.
    pub provenance: Provenance,
    /// What `PATH` says about it, and therefore whether this is the one the user means when
    /// they type the name in any other terminal.
    pub standing: PathStanding,
}

impl ShellInstall {
    /// The install, described in one clause, for a list that has to tell two of them apart.
    ///
    /// `None` when there is nothing to add — which is the ordinary machine with one
    /// install, and the reason A11's row count survives this entry (decision 9).
    pub fn qualifier(&self) -> Option<String> {
        self.provenance.qualifier(&self.program)
    }

    /// The directory holding it, said as a listener would have to read it — the last resort
    /// for telling two installs apart when their provenances say the same thing.
    pub fn directory(&self) -> String {
        self.program
            .parent()
            .map_or_else(String::new, |at| at.display().to_string())
    }
}

/// Where an install came from.
///
/// **A file found somewhere that says nothing is [`Indeterminable`](Self::Indeterminable),
/// which is a state this product ships rather than a gap it fills with a guess** (decision
/// 3). `$PSVersionTable` is the only authoritative answer to "which version is this" and it
/// costs a process; B5.3 already refused to pay that, and that refusal stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// The operating system's own: the file is where the system keeps the programs it
    /// ships, which cannot be uninstalled and cannot be written to without administrator
    /// rights.
    ///
    /// **Called `Windows` until M2, and it was never a Windows fact.** `C:\Windows\system32`
    /// and `/bin` are the same claim about a file — this came with the machine, and no
    /// ordinary user put it there — and a Mac's `/bin/zsh` is as much this as `cmd.exe` is.
    System,
    /// A versioned install directory — `%ProgramFiles%\PowerShell\7` and its twins. The
    /// version is the directory's own name, which is where Windows Terminal reads it from
    /// too, and never from the file.
    Directory {
        /// The directory name, exactly as it stands: `7`, `7-preview`.
        version: String,
        /// Whether that name says preview.
        preview: bool,
    },
    /// A Store package, named by its package family — the identity `CreateProcess` itself
    /// resolves an execution alias through.
    Store {
        /// The package family name, `Microsoft.PowerShell_8wekyb3d8bbwe`.
        family: String,
        /// Whether the family name says preview.
        preview: bool,
    },
    /// The registry's own record of an install: Windows PowerShell's `ApplicationBase`, or
    /// an MSI install of 7 deliberately kept off `PATH`.
    ///
    /// **Right for Windows PowerShell and wrong as a general rule** (decision 2). Measured
    /// 2026-08-27: `HKLM\SOFTWARE\Microsoft\PowerShell\3\PowerShellEngine` exists and
    /// reports 5.1.26100.8875, and
    /// `HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions` does not exist at all on a
    /// machine whose PowerShell 7 came from the Store.
    Registry {
        /// What the registry itself said the version was, when it said anything.
        version: Option<String>,
    },
    /// Found somewhere that says nothing about what it is: a dotnet tool, a scoop shim, a
    /// directory somebody put on `PATH`, a path a user typed.
    Indeterminable,
}

impl Provenance {
    /// The clause that tells this install from another of the same edition, or `None` when
    /// the provenance adds nothing a listener needs.
    ///
    /// The shapes decision 9 names: `PowerShell 7`, `PowerShell 7 (preview)`,
    /// `PowerShell 7 (Microsoft Store)`, `PowerShell 7 (C:\tools\pwsh)`.
    pub fn qualifier(&self, program: &Path) -> Option<String> {
        match self {
            Self::System => None,
            Self::Directory { preview: true, .. } => Some("preview".to_owned()),
            Self::Directory { .. } | Self::Registry { .. } => None,
            Self::Store { preview: true, .. } => Some("Microsoft Store preview".to_owned()),
            Self::Store { .. } => Some("Microsoft Store".to_owned()),
            // **The place, because nothing else about it says anything.** A path is the one
            // fact there is, and a listener comparing two entries can at least hear which
            // directory each came from.
            Self::Indeterminable => program
                .parent()
                .map(|at| at.display().to_string())
                .filter(|at| !at.is_empty()),
        }
    }
}

/// What `PATH` says about an install.
///
/// **`PATH` is kept for the one thing no other source knows: what the name means to this
/// user** (decision 2). Windows Terminal drops `PATH` entirely and enumerates known roots,
/// which is right about everything except this — if the user types `pwsh` in any other
/// terminal, `PATH` decides which one starts, so the entry `PATH` resolves first is marked
/// as the default rather than merely included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStanding {
    /// `PATH` does not name this install at all: it was found by a known root, by the Store
    /// package, or by the registry.
    Absent,
    /// `PATH` names it, and names another one first.
    Named,
    /// The first thing `PATH` resolves this name to, which is what typing the name in any
    /// other terminal starts.
    First,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(program: &str, provenance: Provenance) -> ShellInstall {
        ShellInstall {
            program: PathBuf::from(program),
            provenance,
            standing: PathStanding::Absent,
        }
    }

    /// A path with directories in it, spelled the way the platform running this test spells
    /// one.
    ///
    /// **`C:\tools\pwsh\pwsh.exe` is not a path off Windows — it is one filename.**
    /// `Path::parent` finds no separator in it and answers the empty string, so two tests
    /// below failed on macOS for a reason that had nothing to do with what they are about
    /// (M1). What they *are* about — that an install can say where it lives, and that a
    /// provenance with nothing else to offer says its directory — is true on every platform;
    /// only the spelling of a directory belongs to Windows. So the separators here are the
    /// platform's own and the expectation is built from the same pieces as the path.
    fn under(directories: &[&str], file: &str) -> (PathBuf, String) {
        let directory: PathBuf = directories.iter().collect();
        (directory.join(file), directory.display().to_string())
    }

    /// **The ordinary machine, and the reason A11's row count survives** (decision 9). One
    /// PowerShell 7 in the place PowerShell 7 goes has nothing to add to its own name.
    #[test]
    fn an_install_in_the_place_it_belongs_adds_nothing_to_its_name() {
        let installed = install(
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            Provenance::Directory {
                version: "7".to_owned(),
                preview: false,
            },
        );

        assert_eq!(installed.qualifier(), None);
    }

    /// The shapes decision 9 names, each said the way it will be heard.
    #[test]
    fn each_provenance_says_what_tells_it_from_another() {
        let preview = install(
            r"C:\Program Files\PowerShell\7-preview\pwsh.exe",
            Provenance::Directory {
                version: "7-preview".to_owned(),
                preview: true,
            },
        );
        let store = install(
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe",
            Provenance::Store {
                family: "Microsoft.PowerShell_8wekyb3d8bbwe".to_owned(),
                preview: false,
            },
        );
        let (somewhere, directory) = under(&["tools", "pwsh"], "pwsh.exe");
        let elsewhere = ShellInstall {
            program: somewhere,
            provenance: Provenance::Indeterminable,
            standing: PathStanding::Absent,
        };

        assert_eq!(preview.qualifier().as_deref(), Some("preview"));
        assert_eq!(store.qualifier().as_deref(), Some("Microsoft Store"));
        assert_eq!(
            elsewhere.qualifier().as_deref(),
            Some(directory.as_str()),
            "a provenance with nothing else to say says where it is"
        );
    }

    /// Windows' own shells are not qualified by anything: there is exactly one `cmd.exe`,
    /// and saying where it is would be noise on every machine in the world.
    #[test]
    fn the_shell_windows_ships_is_not_qualified_at_all() {
        let cmd = install(r"C:\Windows\system32\cmd.exe", Provenance::System);

        assert_eq!(cmd.qualifier(), None);
    }

    /// The last resort when two provenances say the same thing: where each one lives.
    #[test]
    fn an_install_can_always_say_which_directory_it_is_in() {
        let (program, directory) = under(&["Program Files", "PowerShell", "7"], "pwsh.exe");
        let installed = ShellInstall {
            program,
            provenance: Provenance::System,
            standing: PathStanding::Absent,
        };

        assert_eq!(installed.directory(), directory);
    }
}
