//! Adapter: what a *Unix* computer actually has, behind acter-core's `ThisComputer` port —
//! the shells an account here may log in to, which of them this account's own is, and every
//! file a named program really resolves to.
//!
//! **One of two adapters behind that port since M2**, the other being
//! [`windows_machine`](crate::windows_machine), with the composition root choosing between
//! them (spec M2, decision 1).
//!
//! **Unix rather than macOS, deliberately.** `/etc/shells` and a passwd entry are POSIX, not
//! Apple's: the file has the same meaning and the same format on Linux, and a Linux lane will
//! reach this file rather than a copy of it. What is macOS-only in M2 is the *signature*
//! adapter beside it, because that one really does read an API only one platform has.
//!
//! # Two sources, and only one of them is a list
//!
//! `/etc/shells` says which shells an account **may** log in to; the passwd entry says which
//! one this account **does**. Both are needed and neither substitutes for the other: the file
//! alone cannot say which entry to start when nobody chooses, and the passwd entry alone
//! hides the six other shells a stock macOS install ships.
//!
//! **The parsing is a pure function over the file's text**, so it is asserted on every
//! platform this crate compiles for rather than only on the one that has the file — M1's
//! lesson, which is that a rule tested on one platform is a rule half-tested. What touches
//! the machine is reading the file, checking that each entry is really there, and asking
//! `getpwuid_r`.

use std::path::{Path, PathBuf};

use acter_core::{
    LoginShell, NoDistributions, PathStanding, Provenance, ShellInstall, ThisComputer,
};
use which::which_all;

/// The file that lists the shells an account on this machine may log in to.
///
/// **The list a Mac itself uses**: it is what `chpass` will accept, and what Terminal.app's
/// own preferences offer. Enumerating a directory instead would offer programs nobody chose
/// to make available (spec M2, decision 3).
const ETC_SHELLS: &str = "/etc/shells";

/// The directories an operating system keeps the programs it ships in.
///
/// **What makes a file [`Provenance::System`] here**, which is the same claim
/// `C:\Windows\system32` carries on the other adapter: it came with the machine, no ordinary
/// user put it there, and on macOS it is under System Integrity Protection so no ordinary
/// user can replace it either.
const SYSTEM_DIRECTORIES: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin", "/usr/libexec"];

/// What this Unix computer has.
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
    /// Every shell `/etc/shells` names that is really there, with this account's own first
    /// and marked.
    ///
    /// **The account's shell is included even when the file does not name it.** A shell
    /// somebody set with `chsh` from a path outside the file is still what a login starts,
    /// and a list that omitted it would offer everything except the one thing Enter does.
    ///
    /// **A file that cannot be read is not the end of the list.** The passwd entry is a
    /// second source, so a machine whose `/etc/shells` is missing still offers the one shell
    /// it is certain of rather than offering nothing.
    fn login_shells(&self) -> Vec<LoginShell> {
        let listed = std::fs::read_to_string(ETC_SHELLS).unwrap_or_default();
        let mine = passwd_shell();
        offered(&listed, mine.as_deref(), |program| program.is_file())
    }

    /// **Nothing, and on a Unix machine that is a fact rather than a refusal** (spec M2,
    /// decision 1). The Windows Subsystem for Linux is not installed here in the plainest
    /// possible sense: it is a Windows feature, and this is not Windows. Nothing asks,
    /// either — the catalogue offers no `Wsl` kind off Windows.
    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
        Err(NoDistributions::NotInstalled)
    }

    /// Every install of this program `PATH` resolves, most preferred first.
    ///
    /// **`PATH` and nothing else, which is the whole of it on Unix.** The other adapter also
    /// walks known roots, a Store package and two registry keys, because a Windows install
    /// can be invisible to `PATH` in four different ways; a Unix program that is not on
    /// `PATH` is a program this user has not made available, and inventing directories to
    /// look in would offer them one they did not choose.
    ///
    /// **Nothing here is a fallback for [`Self::login_shells`]**, and the two answer
    /// different questions: this is "where is the program called `zsh`", and that is "which
    /// shells may an account log in to". The Terminal row is built from the second.
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

    /// The shell this machine's own account logs in to, by name — and `None` for a named far
    /// end, because a Unix machine hosts none.
    ///
    /// **Answering `None` for a name is the truthful answer rather than a decline.** The
    /// argument names a WSL distribution, which is a far end one Windows machine can hold
    /// several of; nothing on a Unix machine answers to a name that way, so there is no
    /// account there to have a shell.
    fn login_shell(&self, far_end: Option<&str>) -> Option<String> {
        match far_end {
            Some(_) => None,
            None => passwd_shell().map(|shell| name_of(&shell)),
        }
    }
}

/// The list the connect dialog is built from, as a pure function over the file's text.
///
/// `exists` is what turns an entry into a file that is really there, passed in rather than
/// called here so that every rule below is asserted without a machine that happens to have
/// the right shells — which is the same seam `catalogue` takes its `has` through.
///
/// **Order is the file's own, with this account's shell moved to the front.** A listener who
/// learns where their shells are must find them in the same places next time, so nothing is
/// sorted; only the one entry that is chosen for them moves.
///
/// **Duplicates are dropped by path.** A file that names `/bin/zsh` twice is a file somebody
/// edited twice, and a panel with two identical entries cannot be navigated by ear.
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
                // **`Absent` rather than `First`, whatever `PATH` says.** What this list is
                // ordered by is the account's own choice, and `PathStanding` is a fact about
                // `PATH` — claiming it here would be answering a question nobody asked with
                // an answer nobody measured.
                standing: PathStanding::Absent,
                program,
            },
        })
        .collect()
}

/// The paths one `/etc/shells` names, in the order it names them.
///
/// The format is one absolute path per line, `#` starting a comment, and blank lines
/// ignored — which is what every implementation of this file agrees on and what the file on
/// a stock macOS install actually contains.
fn entries(listed: &str) -> impl Iterator<Item = PathBuf> + '_ {
    listed
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        // **Absolute paths only.** A relative entry cannot be started reliably and cannot be
        // verified at all, since which file it means depends on where Acter happens to be.
        .filter(|entry| entry.starts_with('/'))
        .map(PathBuf::from)
}

/// Where this file came from, which on Unix is one question: did the operating system ship
/// it, or did somebody install it.
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

/// The file's own name, which is what a user of this machine calls this shell.
fn name_of(program: &Path) -> String {
    program
        .file_name()
        .map_or_else(String::new, |file| file.to_string_lossy().into_owned())
}

/// The shell this account's passwd entry names, or `None` when nothing honest can be said.
///
/// **`getpwuid_r` rather than `$SHELL`** (spec M2, decision 3). `$SHELL` is what started the
/// process that started Acter: inherited through a launcher, edited by anybody's dotfile, and
/// simply absent when the window is opened from the Dock. The passwd entry is what `login`
/// itself reads, so it is what "the shell this account logs in to" means.
#[cfg(unix)]
fn passwd_shell() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    // Long enough for any passwd entry a directory service returns; `getpwuid_r` says so
    // itself by failing with `ERANGE` rather than truncating, which is the arm below.
    const BUFFER: usize = 4096;

    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0_i8; BUFFER];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry` and `found` are owned here and outlive the call, `buffer` is the length
    // passed with it, and the reentrant form writes into what it is given rather than into
    // static storage. A non-zero return or a null `found` is the account not being answered
    // for, which the `?` below turns into `None`.
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
    // An account with no shell at all is a real thing — a service account with `/usr/bin/
    // false`, or an empty field — and an empty path is not something to start.
    (!shell.as_os_str().is_empty()).then_some(shell)
}

/// No passwd file, so no account here logs in to a shell.
///
/// **A gated function rather than a gated module**, which is ARCHITECTURE's rule about size:
/// everything else in this file is `/etc/shells` and paths, and both are as readable on
/// Windows as anywhere. Gating the whole adapter would take its parsing tests off Windows CI
/// for the sake of one call.
#[cfg(not(unix))]
fn passwd_shell() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stock macOS `/etc/shells`, as it stands on the machine M2 was measured on.
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

    /// The whole file, in the file's own order, with nothing invented and no comment in it.
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

    /// **The rule the row is built on**: what Enter starts is first, and says so.
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

    /// The rest of the list keeps the file's order behind it, so installing or choosing
    /// something moves one entry rather than rearranging a panel somebody has learned.
    #[test]
    fn moving_the_default_to_the_front_does_not_reorder_the_rest() {
        let offered = offered(STOCK, Some(Path::new("/bin/ksh")), everything);

        assert_eq!(
            offered.iter().map(LoginShell::name).collect::<Vec<_>>(),
            ["ksh", "bash", "csh", "dash", "sh", "tcsh", "zsh"]
        );
    }

    /// **A shell set with `chsh` from outside the file is still what a login starts.** A list
    /// that omitted it would offer everything except the one thing Enter does.
    #[test]
    fn an_account_shell_the_file_does_not_name_is_offered_anyway() {
        let offered = offered(STOCK, Some(Path::new("/opt/homebrew/bin/fish")), everything);

        let first = offered.first().expect("the list is not empty");
        assert_eq!(first.name(), "fish");
        assert!(first.default);
        assert_eq!(offered.len(), 8, "and the file's seven are still there");
    }

    /// Comments, blank lines and anything that is not an absolute path are not shells.
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

    /// A file edited twice names one shell, not two: a panel with two identical entries
    /// cannot be navigated by ear.
    #[test]
    fn a_shell_named_twice_is_offered_once() {
        let offered = offered("/bin/zsh\n/bin/bash\n/bin/zsh\n", None, everything);

        assert_eq!(
            offered.iter().map(LoginShell::name).collect::<Vec<_>>(),
            ["zsh", "bash"]
        );
    }

    /// The account's own shell is not listed twice either, which is the ordinary case: it is
    /// in the file *and* in the passwd entry.
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

    /// **A named shell that is not there is not offered.** The file says what an account
    /// *may* log in to, which is not the same as what is installed — an entry left behind by
    /// an uninstalled package would otherwise be a row that fails when it is chosen.
    #[test]
    fn a_shell_the_file_names_and_the_machine_does_not_have_is_not_offered() {
        let offered = offered(STOCK, None, |program| program != Path::new("/bin/ksh"));

        assert!(
            !offered.iter().any(|shell| shell.name() == "ksh"),
            "a file that is not there is not a shell to offer"
        );
        assert_eq!(offered.len(), 6);
    }

    /// A machine with no readable file and no passwd answer offers nothing, which the kind's
    /// own instructions are what explain.
    #[test]
    fn a_machine_that_says_nothing_offers_nothing() {
        assert!(offered("", None, everything).is_empty());
    }

    /// Where a shell came from, which is what a listener would be told if two of them ever
    /// needed telling apart.
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

    /// **The real machine, asked the way the connect list asks it** — because every test
    /// above hands the parsing its own text, and none of them proves that this adapter can
    /// read a file and a passwd entry at all.
    ///
    /// macOS only: it is the platform M2 built this for and the one whose `/etc/shells` is
    /// guaranteed to hold something. A Linux CI runner may legitimately have a different
    /// file, and this asserts what is true of every Mac rather than what happened to be true
    /// of one.
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

    /// The passwd entry itself, which is the half `$SHELL` would have got wrong.
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

    /// **The WSL question is answered with a fact rather than declined** (spec M2,
    /// decision 1), and the fact is the plainest one there is.
    #[test]
    fn a_unix_machine_has_no_wsl_because_it_is_not_windows() {
        assert_eq!(
            UnixMachine::new().wsl_distributions(),
            Err(NoDistributions::NotInstalled)
        );
    }

    /// And the per-connection question, which on this machine has no far end to be about.
    #[test]
    fn nothing_on_a_unix_machine_answers_to_a_far_ends_name() {
        assert_eq!(UnixMachine::new().login_shell(Some("Ubuntu")), None);
    }
}
