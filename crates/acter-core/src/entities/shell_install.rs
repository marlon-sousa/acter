//! Entity/value: one install of a shell that this machine actually has — the file itself,
//! where it came from, and what `PATH` says about it.
//!
//! Availability answers with the path it found, that path is what is verified, and that
//! path is what is started: checking a signature and starting a different file `PATH`
//! resolves a moment later would be theatre.
//!
//! What identifies an install is where it came from, never what the file says about
//! itself. Measured against a real machine: `powershell.exe` reports FileVersion
//! `10.0.26100.8875` — the Windows build, not 5.1 — so a design that read the version
//! resource would be wrong for one of the two editions in exactly the direction that
//! matters. Windows Terminal reaches the same conclusion independently: it takes the
//! version from the directory name or the package identity and never opens the file.

use std::path::{Path, PathBuf};

/// One file this machine can start, found once and started as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInstall {
    /// This is what gets verified and what gets started: the window between the check
    /// and the spawn does not close completely, but narrows from "any file on `PATH`" to
    /// "this file, moved or replaced in the seconds since".
    pub program: PathBuf,
    /// Where it came from, which is what tells two installs of the same edition apart.
    pub provenance: Provenance,
    /// What `PATH` says about it, and therefore whether this is the one the user means when
    /// they type the name in any other terminal.
    pub standing: PathStanding,
}

impl ShellInstall {
    /// The install, described in one clause, for a list that has to tell two of them apart.
    /// `None` when there is nothing to add, the ordinary case of one install.
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
/// A file found somewhere that says nothing is [`Indeterminable`](Self::Indeterminable),
/// a state this product ships rather than a gap it fills with a guess: `$PSVersionTable`
/// is the only authoritative answer to "which version is this" and it costs a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// The operating system's own: the file is where the system keeps the programs it
    /// ships, which cannot be uninstalled and cannot be written to without administrator
    /// rights.
    ///
    /// `C:\Windows\system32` and `/bin` are the same claim about a file — it came with
    /// the machine, and no ordinary user put it there — and a Mac's `/bin/zsh` is as much
    /// this as `cmd.exe` is.
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
    /// Right for Windows PowerShell and wrong as a general rule. Measured against a real
    /// machine: `HKLM\SOFTWARE\Microsoft\PowerShell\3\PowerShellEngine` exists and reports
    /// 5.1.26100.8875, while `HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions`
    /// does not exist at all on a machine whose PowerShell 7 came from the Store.
    Registry {
        /// What the registry itself said the version was, when it said anything.
        version: Option<String>,
    },
    /// Found somewhere that says nothing about what it is: a dotnet tool, a scoop shim, a
    /// directory somebody put on `PATH`, a path a user typed.
    Indeterminable,
}

impl Provenance {
    /// The clause that tells this install from another of the same edition, or `None`
    /// when the provenance adds nothing a listener needs.
    pub fn qualifier(&self, program: &Path) -> Option<String> {
        match self {
            Self::System => None,
            Self::Directory { preview: true, .. } => Some("preview".to_owned()),
            Self::Directory { .. } | Self::Registry { .. } => None,
            Self::Store { preview: true, .. } => Some("Microsoft Store preview".to_owned()),
            Self::Store { .. } => Some("Microsoft Store".to_owned()),
            // The place, because nothing else about it says anything: a listener
            // comparing two entries can at least hear which directory each came from.
            Self::Indeterminable => program
                .parent()
                .map(|at| at.display().to_string())
                .filter(|at| !at.is_empty()),
        }
    }
}

/// What `PATH` says about an install.
///
/// `PATH` is kept for the one thing no other source knows: what the name means to this
/// user. If the user types `pwsh` in any other terminal, `PATH` decides which one starts,
/// so the entry `PATH` resolves first is marked as the default rather than merely
/// included.
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

    /// `Path::parent` finds no separator in a Windows-style path spelled on macOS and
    /// answers the empty string, so the separators here are built from the platform's own.
    fn under(directories: &[&str], file: &str) -> (PathBuf, String) {
        let directory: PathBuf = directories.iter().collect();
        (directory.join(file), directory.display().to_string())
    }

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

    #[test]
    fn the_shell_windows_ships_is_not_qualified_at_all() {
        let cmd = install(r"C:\Windows\system32\cmd.exe", Provenance::System);

        assert_eq!(cmd.qualifier(), None);
    }

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
