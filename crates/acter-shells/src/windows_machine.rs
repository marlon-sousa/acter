//! Adapter: what a Windows computer has, behind acter-core's `ThisComputer` port.
//!
//! The only module in this crate that starts a process; its tests must not start `wsl.exe`.

use std::env::{var_os, vars_os};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use acter_core::{
    LoginShell, NoDistributions, PathStanding, Provenance, ShellInstall, ThisComputer,
};
use which::which_all;

use crate::wsl::distributions::{decode_utf16le, distributions};
use crate::wsl::login_shell;

mod roots;

/// `-q` drops the header line and the `(Default)` suffix, leaving one bare name per line.
const LIST: (&str, [&str; 2]) = ("wsl.exe", ["-l", "-q"]);

const RUN: (&str, &str, &str) = ("wsl.exe", "-d", "--");

const POSIX_SHELL: &str = "sh";

/// Ubuntu 24.04 under WSL 2.5.7.0 answers `wsl.exe -d Ubuntu -- sh -c` in 141 to 206 ms warm
/// and 5.22 to 6.30 s cold; `wsl.exe -l -q` does not boot a distribution.
const PATIENCE: Duration = Duration::from_secs(12);

const TICK: Duration = Duration::from_millis(25);

const WINDOWS_APPS: &str = "WindowsApps";

const PUBLISHER: &str = "__";

const POWERSHELL: &str = "PowerShell";

#[derive(Debug, Default)]
pub struct WindowsMachine;

impl WindowsMachine {
    pub fn new() -> Self {
        Self
    }
}

impl ThisComputer for WindowsMachine {
    /// Empty: Windows has no `/etc/shells`, and an account does not log in to a shell.
    fn login_shells(&self) -> Vec<LoginShell> {
        Vec::new()
    }

    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
        let (program, flags) = LIST;
        let listed = Command::new(program)
            .args(flags)
            .output()
            .map_err(|_| NoDistributions::NotInstalled)?;

        if !listed.status.success() {
            return Err(NoDistributions::NotWorking {
                detail: refusal(&listed.stdout, &listed.stderr),
            });
        }

        let names = distributions(&listed.stdout);
        if names.is_empty() {
            return Err(NoDistributions::NoneInstalled);
        }
        Ok(names)
    }

    fn installs(&self, program: &str) -> Vec<ShellInstall> {
        let mut found: Vec<ShellInstall> = Vec::new();
        for (index, candidate) in on_path(program).into_iter().enumerate() {
            let standing = if index == 0 {
                PathStanding::First
            } else {
                PathStanding::Named
            };
            keep(&mut found, resolve(&candidate, standing));
        }
        for candidate in roots::known(program) {
            keep(&mut found, resolve(&candidate, PathStanding::Absent));
        }
        for (candidate, version) in roots::registered(program) {
            keep(
                &mut found,
                resolve(&candidate, PathStanding::Absent).map(|install| ShellInstall {
                    provenance: match install.provenance {
                        Provenance::Indeterminable => Provenance::Registry { version },
                        known => known,
                    },
                    ..install
                }),
            );
        }
        found
    }

    /// `None` when `wsl.exe` cannot start, the answer is not a shell name, or `PATIENCE` passes.
    fn login_shell(&self, distribution: Option<&str>) -> Option<String> {
        let (program, flag, separator) = RUN;
        let mut asking = Command::new(program);
        if let Some(name) = distribution {
            asking.args([flag, name]);
        }
        asking.args([separator, POSIX_SHELL, "-c", login_shell::ASK]);

        login_shell::read(&answered_within(asking, PATIENCE)?)
    }
}

/// What a command wrote to standard output, or `None` if it could not start or outlived
/// `patience`, in which case it is killed.
///
/// Standard output is drained on its own thread because a child that fills the pipe never exits.
fn answered_within(mut command: Command, patience: Duration) -> Option<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut pipe = child.stdout.take()?;
    let reading = thread::spawn(move || {
        let mut said = Vec::new();
        let _ = pipe.read_to_end(&mut said);
        said
    });

    let deadline = Instant::now() + patience;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return reading.join().ok(),
            Ok(None) if Instant::now() < deadline => thread::sleep(TICK),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn on_path(program: &str) -> Vec<PathBuf> {
    which_all(program)
        .map(|found| found.collect())
        .unwrap_or_default()
}

/// An alias that does not resolve stays the program, and verification reports it unverifiable.
fn resolve(candidate: &Path, standing: PathStanding) -> Option<ShellInstall> {
    if !candidate.is_file() {
        return None;
    }
    let program = package(candidate).unwrap_or_else(|| candidate.to_path_buf());
    Some(ShellInstall {
        provenance: provenance(&program),
        program,
        standing,
    })
}

/// `None` for a file that is not an execution alias, or whose package file is missing.
#[cfg(windows)]
fn package(candidate: &Path) -> Option<PathBuf> {
    crate::signature_target(candidate)
        .map(|link| link.program)
        .filter(|program| program.is_file())
}

#[cfg(not(windows))]
fn package(_candidate: &Path) -> Option<PathBuf> {
    None
}

/// The first install found for a file wins, so `PATH`'s standing survives a known root finding
/// it again. On Windows 11 Pro 26200, `where pwsh` lists the Store package file and the execution
/// alias that resolves to it.
fn keep(found: &mut Vec<ShellInstall>, install: Option<ShellInstall>) {
    let Some(install) = install else {
        return;
    };
    if found
        .iter()
        .any(|have| same_file(&have.program, &install.program))
    {
        return;
    }
    found.push(install);
}

fn same_file(one: &Path, other: &Path) -> bool {
    let settle = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .as_os_str()
            .to_string_lossy()
            .to_lowercase()
    };
    settle(one) == settle(other)
}

/// Never reads the version resource: Windows PowerShell 5.1's `powershell.exe` reports
/// FileVersion 10.0.26100.8875, the Windows build.
fn provenance(program: &Path) -> Provenance {
    let parts: Vec<String> = program
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();

    if let Some(at) = parts.iter().position(|part| part == WINDOWS_APPS)
        && let Some(full_name) = parts.get(at + 1)
        && let Some(family) = family(full_name)
    {
        return Provenance::Store {
            preview: family.to_lowercase().contains("preview"),
            family,
        };
    }
    if in_windows(&parts) {
        return Provenance::System;
    }
    if let Some(at) = parts.iter().position(|part| part == POWERSHELL)
        && let Some(version) = parts.get(at + 1)
        && program.parent().map(Path::to_path_buf) == Some(root_of(program, at + 1))
    {
        return Provenance::Directory {
            preview: version.to_lowercase().contains("preview"),
            version: version.clone(),
        };
    }
    Provenance::Indeterminable
}

fn family(full_name: &str) -> Option<String> {
    let (before, publisher) = full_name.split_once(PUBLISHER)?;
    let name = before.split('_').next()?;
    if name.is_empty() || publisher.is_empty() {
        return None;
    }
    Some(format!("{name}_{publisher}"))
}

fn in_windows(parts: &[String]) -> bool {
    let system = var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let system: Vec<String> = system
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let under: Vec<String> = parts.iter().map(|part| part.to_lowercase()).collect();
    under.len() > system.len() && under.starts_with(&system)
}

fn root_of(program: &Path, at: usize) -> PathBuf {
    program.components().take(at + 1).collect()
}

fn refusal(stdout: &[u8], stderr: &[u8]) -> String {
    let said = decode_utf16le(stdout);
    let said = if said.trim().is_empty() {
        decode_utf16le(stderr)
    } else {
        said
    };
    let said = said.trim();
    if said.is_empty() {
        "It gave no reason.".to_owned()
    } else {
        said.to_owned()
    }
}

fn environment() -> Vec<(String, PathBuf)> {
    vars_os()
        .filter_map(|(name, value)| Some((name.to_str()?.to_uppercase(), PathBuf::from(value))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Joined from parts because `Path` splits only on the host's separator.
    fn at(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn a_file_under_a_store_package_is_named_by_its_package_family() {
        let provenance = provenance(&at(&[
            "Program Files",
            WINDOWS_APPS,
            "Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe",
            "pwsh.exe",
        ]));

        assert_eq!(
            provenance,
            Provenance::Store {
                family: "Microsoft.PowerShell_8wekyb3d8bbwe".to_owned(),
                preview: false,
            }
        );
    }

    #[test]
    fn a_preview_package_says_so() {
        let provenance = provenance(&at(&[
            "Program Files",
            WINDOWS_APPS,
            "Microsoft.PowerShellPreview_7.7.0.0_x64__8wekyb3d8bbwe",
            "pwsh.exe",
        ]));

        assert_eq!(
            provenance,
            Provenance::Store {
                family: "Microsoft.PowerShellPreview_8wekyb3d8bbwe".to_owned(),
                preview: true,
            }
        );
    }

    #[test]
    fn an_msi_install_is_named_by_the_directory_it_was_installed_into() {
        assert_eq!(
            provenance(&at(&["Program Files", POWERSHELL, "7", "pwsh.exe"])),
            Provenance::Directory {
                version: "7".to_owned(),
                preview: false,
            }
        );
        assert_eq!(
            provenance(&at(&["Program Files", POWERSHELL, "7-preview", "pwsh.exe"])),
            Provenance::Directory {
                version: "7-preview".to_owned(),
                preview: true,
            }
        );
    }

    #[test]
    fn a_file_somewhere_that_says_nothing_is_reported_as_saying_nothing() {
        for anywhere in [
            at(&["Users", "someone", ".dotnet", "tools", "pwsh.exe"]),
            at(&["Users", "someone", "scoop", "shims", "pwsh.exe"]),
            at(&["tools", "pwsh", "pwsh.exe"]),
            at(&[
                "Program Files",
                POWERSHELL,
                "7",
                "Modules",
                "Something",
                "pwsh.exe",
            ]),
        ] {
            assert_eq!(
                provenance(&anywhere),
                Provenance::Indeterminable,
                "{} says nothing about itself",
                anywhere.display()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_shells_windows_ships_come_from_windows() {
        let system = var_os("SystemRoot").expect("Windows always sets this");
        let system = PathBuf::from(system);

        assert_eq!(
            provenance(&system.join("system32").join("cmd.exe")),
            Provenance::System
        );
        assert_eq!(
            provenance(
                &system
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            ),
            Provenance::System,
            "case is not what tells a directory apart on this platform"
        );
    }

    #[test]
    fn something_that_is_not_a_package_full_name_names_no_family() {
        assert_eq!(family("Microsoft.PowerShell"), None);
        assert_eq!(family("__8wekyb3d8bbwe"), None);
        assert_eq!(family("Microsoft.PowerShell_7.6.5.0_x64__"), None);
        assert_eq!(
            family("Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe").as_deref(),
            Some("Microsoft.PowerShell_8wekyb3d8bbwe")
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_shell_every_windows_machine_has_resolves_to_a_file_that_is_there() {
        let machine = WindowsMachine::new();

        for named in ["cmd.exe", "cmd"] {
            let installs = machine.installs(named);
            let first = installs
                .first()
                .unwrap_or_else(|| panic!("{named} is on every machine"));

            assert!(first.program.is_file(), "and it is a file: {first:?}");
            assert_eq!(
                first.standing,
                PathStanding::First,
                "PATH names it, so it is what typing the name starts"
            );
            assert_eq!(first.provenance, Provenance::System);
        }
        assert!(
            machine
                .installs("acter-no-such-program-exists.exe")
                .is_empty(),
            "and a name nothing resolves has no installs at all"
        );
    }

    #[cfg(windows)]
    #[test]
    fn one_file_found_twice_is_listed_once() {
        let installs = WindowsMachine::new().installs("cmd.exe");

        let mut seen: Vec<String> = installs
            .iter()
            .map(|install| install.program.to_string_lossy().to_lowercase())
            .collect();
        seen.sort();
        let listed = seen.len();
        seen.dedup();

        assert_eq!(seen.len(), listed, "no file appears twice: {installs:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_program_named_by_its_full_path_is_the_only_install_of_itself() {
        let cmd = PathBuf::from(var_os("SystemRoot").expect("Windows sets this"))
            .join("system32")
            .join("cmd.exe");

        let installs = WindowsMachine::new().installs(&cmd.to_string_lossy());

        assert_eq!(installs.len(), 1);
        assert!(
            installs[0]
                .program
                .to_string_lossy()
                .to_lowercase()
                .ends_with("cmd.exe"),
            "{installs:?}"
        );
    }

    #[test]
    fn a_refusal_is_read_back_in_wsls_own_words_from_the_stream_it_wrote_them_to() {
        let mut said = Vec::new();
        for unit in "There is no distribution with the supplied name.".encode_utf16() {
            said.extend_from_slice(&unit.to_le_bytes());
        }

        assert_eq!(
            refusal(&said, &[]),
            "There is no distribution with the supplied name."
        );
    }

    #[test]
    fn a_refusal_with_nothing_said_still_finishes_the_sentence_it_is_appended_to() {
        let spoken = NoDistributions::NotWorking {
            detail: refusal(&[], &[]),
        }
        .to_string();

        assert!(spoken.ends_with('.'));
        assert!(spoken.ends_with("It gave no reason."));
    }
}
