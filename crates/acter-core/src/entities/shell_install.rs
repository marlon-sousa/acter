//! Entity/value: one install of a shell that this machine actually has — the file itself,
//! where it came from, and what `PATH` says about it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellInstall {
    /// The file verified is the file started; never resolve the name through `PATH` again.
    pub program: PathBuf,
    pub provenance: Provenance,
    pub standing: PathStanding,
}

impl ShellInstall {
    /// `None` when nothing tells this install apart, as on a machine with one install.
    pub fn qualifier(&self) -> Option<String> {
        self.provenance.qualifier(&self.program)
    }

    pub fn directory(&self) -> String {
        self.program
            .parent()
            .map_or_else(String::new, |at| at.display().to_string())
    }
}

/// Where an install came from, read off its location and never off the file's version
/// resource; see `provenance` in acter-shells' windows_machine.rs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// `C:\Windows\system32`, `/bin`: shipped with the operating system.
    System,
    Directory {
        /// The directory name as it stands: `7`, `7-preview`.
        version: String,
        preview: bool,
    },
    Store {
        /// `Microsoft.PowerShell_8wekyb3d8bbwe`.
        family: String,
        preview: bool,
    },
    Registry {
        /// `None` when the registry gave no version.
        version: Option<String>,
    },
    /// A dotnet tool, a scoop shim, a directory on `PATH`, a typed path.
    Indeterminable,
}

impl Provenance {
    pub fn qualifier(&self, program: &Path) -> Option<String> {
        match self {
            Self::System => None,
            Self::Directory { preview: true, .. } => Some("preview".to_owned()),
            Self::Directory { .. } | Self::Registry { .. } => None,
            Self::Store { preview: true, .. } => Some("Microsoft Store preview".to_owned()),
            Self::Store { .. } => Some("Microsoft Store".to_owned()),
            Self::Indeterminable => program
                .parent()
                .map(|at| at.display().to_string())
                .filter(|at| !at.is_empty()),
        }
    }
}

/// What `PATH` says about an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStanding {
    /// Found by a known root, the Store package or the registry instead.
    Absent,
    /// `PATH` names another install first.
    Named,
    /// What typing the name in any other terminal starts.
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
