//! Adapter: what a Unix computer has, behind acter-core's `ThisComputer` port.

use std::path::{Path, PathBuf};

use acter_core::{
    LoginShell, NoDistributions, PathStanding, Provenance, ShellInstall, ThisComputer,
};
use which::which_all;

const ETC_SHELLS: &str = "/etc/shells";

const SYSTEM_DIRECTORIES: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin", "/usr/libexec"];

pub struct UnixMachine;

impl UnixMachine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnixMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl ThisComputer for UnixMachine {
    fn login_shells(&self) -> Vec<LoginShell> {
        let listed = std::fs::read_to_string(ETC_SHELLS).unwrap_or_default();
        let mine = passwd_shell();
        offered(&listed, mine.as_deref(), |program| program.is_file())
    }

    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
        Err(NoDistributions::NotInstalled)
    }

    fn installs(&self, program: &str) -> Vec<ShellInstall> {
        which_all(program)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, program)| ShellInstall {
                provenance: provenance(&program),
                program,
                standing: if index == 0 {
                    PathStanding::First
                } else {
                    PathStanding::Named
                },
            })
            .collect()
    }

    fn login_shell(&self, far_end: Option<&str>) -> Option<String> {
        match far_end {
            Some(_) => None,
            None => passwd_shell().map(|shell| name_of(&shell)),
        }
    }
}

fn offered(listed: &str, mine: Option<&Path>, exists: impl Fn(&Path) -> bool) -> Vec<LoginShell> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in mine
        .map(Path::to_path_buf)
        .into_iter()
        .chain(entries(listed))
    {
        if !paths.contains(&entry) && exists(&entry) {
            paths.push(entry);
        }
    }
    paths
        .into_iter()
        .map(|program| LoginShell {
            default: Some(program.as_path()) == mine,
            install: ShellInstall {
                provenance: provenance(&program),
                standing: PathStanding::Absent,
                program,
            },
        })
        .collect()
}

fn entries(listed: &str) -> impl Iterator<Item = PathBuf> + '_ {
    listed
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|entry| entry.starts_with('/'))
        .map(PathBuf::from)
}

fn provenance(program: &Path) -> Provenance {
    match program.parent() {
        Some(directory)
            if SYSTEM_DIRECTORIES
                .iter()
                .any(|system| directory == Path::new(system)) =>
        {
            Provenance::System
        }
        _ => Provenance::Indeterminable,
    }
}

fn name_of(program: &Path) -> String {
    program
        .file_name()
        .map_or_else(String::new, |file| file.to_string_lossy().into_owned())
}

#[cfg(unix)]
fn passwd_shell() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    // A longer entry makes `getpwuid_r` fail with `ERANGE`, which answers `None`.
    const BUFFER: usize = 4096;

    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0_i8; BUFFER];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry`, `buffer` and `found` outlive the call, and `buffer.len()` is the
    // buffer's length.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &raw mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &raw mut found,
        )
    };
    if status != 0 || found.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    // SAFETY: `pw_shell` points into `buffer`, which is still alive, and `getpwuid_r`
    // guarantees it is a null-terminated string.
    let shell = unsafe { CStr::from_ptr(entry.pw_shell) };
    let shell = PathBuf::from(OsStr::from_bytes(shell.to_bytes()));
    (!shell.as_os_str().is_empty()).then_some(shell)
}

#[cfg(not(unix))]
fn passwd_shell() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOCK: &str = "# List of acceptable shells for chpass(1).\n\
                         # Ftpd will not allow users to connect who are not using\n\
                         # one of these shells.\n\
                         \n\
                         /bin/bash\n\
                         /bin/csh\n\
                         /bin/dash\n\
                         /bin/ksh\n\
                         /bin/sh\n\
                         /bin/tcsh\n\
                         /bin/zsh\n";

    fn everything(_: &Path) -> bool {
        true
    }

    #[test]
    fn the_shells_offered_are_the_ones_the_file_names() {
        let offered = offered(STOCK, None, everything);

        assert_eq!(
            offered.iter().map(LoginShell::name).collect::<Vec<_>>(),
            ["bash", "csh", "dash", "ksh", "sh", "tcsh", "zsh"],
            "seven shells, in the order a stock macOS install lists them"
        );
        assert!(
            offered.iter().all(|shell| !shell.default),
            "and nothing is the default when the account did not say"
        );
    }

    #[test]
    fn the_accounts_own_shell_is_first_and_marked() {
        let offered = offered(STOCK, Some(Path::new("/bin/zsh")), everything);

        let first = offered.first().expect("the list is not empty");
        assert_eq!(first.name(), "zsh");
        assert!(first.default, "the account's own shell says it is the one");
        assert_eq!(
            offered.iter().filter(|shell| shell.default).count(),
            1,
            "and exactly one entry can be the one"
        );
    }

    #[test]
    fn moving_the_default_to_the_front_does_not_reorder_the_rest() {
        let offered = offered(STOCK, Some(Path::new("/bin/ksh")), everything);

        assert_eq!(
            offered.iter().map(LoginShell::name).collect::<Vec<_>>(),
            ["ksh", "bash", "csh", "dash", "sh", "tcsh", "zsh"]
        );
    }

    #[test]
    fn an_account_shell_the_file_does_not_name_is_offered_anyway() {
        let offered = offered(STOCK, Some(Path::new("/opt/homebrew/bin/fish")), everything);

        let first = offered.first().expect("the list is not empty");
        assert_eq!(first.name(), "fish");
        assert!(first.default);
        assert_eq!(offered.len(), 8, "and the file's seven are still there");
    }

    #[test]
    fn only_absolute_paths_are_shells() {
        let listed = "# a comment\n\n  \nzsh\n../bin/zsh\n/bin/zsh  # the real one\n";

        let offered = offered(listed, None, everything);

        assert_eq!(
            offered
                .iter()
                .map(|shell| shell.program().display().to_string())
                .collect::<Vec<_>>(),
            ["/bin/zsh"],
            "a bare name and a relative path name no file this can verify"
        );
    }

    #[test]
    fn a_shell_named_twice_is_offered_once() {
        let offered = offered("/bin/zsh\n/bin/bash\n/bin/zsh\n", None, everything);

        assert_eq!(
            offered.iter().map(LoginShell::name).collect::<Vec<_>>(),
            ["zsh", "bash"]
        );
    }

    #[test]
    fn the_account_shell_is_not_listed_twice_for_being_in_both() {
        let offered = offered(STOCK, Some(Path::new("/bin/bash")), everything);

        assert_eq!(offered.len(), 7, "seven shells, not eight");
        assert_eq!(
            offered
                .iter()
                .filter(|shell| shell.name() == "bash")
                .count(),
            1
        );
    }

    #[test]
    fn a_shell_the_file_names_and_the_machine_does_not_have_is_not_offered() {
        let offered = offered(STOCK, None, |program| program != Path::new("/bin/ksh"));

        assert!(
            !offered.iter().any(|shell| shell.name() == "ksh"),
            "a file that is not there is not a shell to offer"
        );
        assert_eq!(offered.len(), 6);
    }

    #[test]
    fn a_machine_that_says_nothing_offers_nothing() {
        assert!(offered("", None, everything).is_empty());
    }

    #[test]
    fn a_shell_the_system_ships_says_so_and_one_somebody_installed_does_not() {
        assert_eq!(provenance(Path::new("/bin/zsh")), Provenance::System);
        assert_eq!(provenance(Path::new("/usr/bin/zsh")), Provenance::System);
        assert_eq!(
            provenance(Path::new("/opt/homebrew/bin/fish")),
            Provenance::Indeterminable,
            "a shell somebody installed is not one the system ships"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_mac_answers_with_shells_it_really_has() {
        let offered = UnixMachine::new().login_shells();

        assert!(!offered.is_empty(), "every Mac has shells to log in to");
        assert!(
            offered.iter().any(|shell| shell.name() == "zsh"),
            "including zsh, which macOS has shipped as the default since Catalina"
        );
        assert!(
            offered.iter().all(|shell| shell.program().is_file()),
            "and every one of them is a file that is really there"
        );
        assert_eq!(
            offered.iter().filter(|shell| shell.default).count(),
            1,
            "exactly one is the shell this account logs in to"
        );
        assert!(
            offered[0].default,
            "and it is first, which is what the row's own id is taken from"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_mac_names_the_shell_its_account_logs_in_to() {
        let named = UnixMachine::new()
            .login_shell(None)
            .expect("an account on a Mac logs in to something");

        assert!(!named.is_empty());
        assert!(
            !named.contains('/'),
            "it is a name rather than a path: {named}"
        );
    }

    #[test]
    fn a_unix_machine_has_no_wsl_because_it_is_not_windows() {
        assert_eq!(
            UnixMachine::new().wsl_distributions(),
            Err(NoDistributions::NotInstalled)
        );
    }

    #[test]
    fn nothing_on_a_unix_machine_answers_to_a_far_ends_name() {
        assert_eq!(UnixMachine::new().login_shell(Some("Ubuntu")), None);
    }
}
