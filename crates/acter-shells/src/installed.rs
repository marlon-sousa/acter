//! Adapter: what this computer actually has, behind acter-core's `InstalledShells` port —
//! the WSL distributions `wsl.exe` reports, every file a named program really resolves to,
//! and which shell a distribution's own account runs.
//!
//! **The only module in this crate that starts a process.** Everything else here is
//! knowledge: what `cmd.exe` is started with is the same answer on every Windows machine,
//! and which distributions are installed is the same answer on no two machines at all. That
//! difference is ARCHITECTURE's classifying question, and it is why this is the one file with
//! a `Command` in it.
//!
//! **A workspace test run must never reach the WSL half.** `cargo test --workspace` spawns
//! no process, which is why the decode it depends on lives in
//! [`distributions`](crate::wsl::distributions) as a pure function over captured bytes and is
//! tested there, and why B5.5's reading of a passwd entry lives beside it in
//! [`login_shell`](crate::wsl::login_shell). Discovery is different: looking a program up
//! starts nothing, so the tests here ask the real machine about the shell every Windows
//! machine has — as they did before B5.7, and for the same reason.
//!
//! # Since B5.5: the one call here that is given up on
//!
//! Both WSL questions start `wsl.exe`, and only one of them can afford to wait. Listing
//! distributions happens while a user reads a dialog; asking a distribution what shell it
//! runs happens in the seconds before there is a prompt, which are already the worst seconds
//! in this product (roadmap 23.7). So that one runs under a deadline and every way of
//! failing is the same `None` — advisory, never a gate (spec B5.5, decision 3).
//!
//! # Since B5.7: the answer is the files, not a boolean
//!
//! `is_available` walked `PATH` and answered whether *a* file existed; `LocalPty` then
//! spawned the program by name and Windows resolved it a second time. Nothing guaranteed the
//! two resolutions landed on the same file, which makes `PATH`-order hijacking cheap and
//! would make any signature check theatre. So this resolves once and hands back the paths
//! (spec B5.7, decision 1).
//!
//! **`PATH` is one source and not the whole of it** (decision 2). An MSI install with "add to
//! PATH" unchecked is invisible to it; scoop and chocolatey put shims on it that are not the
//! program; and on the developer's machine, measured 2026-08-27, `where pwsh` gives *two*
//! hits for one install — the Store package directory and the execution alias beside it. What
//! `PATH` alone knows is what the name means to this user, which is why it is kept and why
//! the entry it resolves first is marked as the default.

use std::env::{var_os, vars_os};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use acter_core::{InstalledShells, NoDistributions, PathStanding, Provenance, ShellInstall};
use which::which_all;

use crate::wsl::distributions::{decode_utf16le, distributions};
use crate::wsl::login_shell;

mod roots;

/// The client asked what is installed, and the flags that make it answer with names and
/// nothing else.
///
/// `-l -q` rather than `--list --quiet` for no reason except that this is what was
/// measured; `-q` is what strips the header line and the `(Default)` suffix, leaving one
/// bare name per line.
const LIST: (&str, [&str; 2]) = ("wsl.exe", ["-l", "-q"]);

/// The client, the flag that points it at one distribution, and the separator after which
/// everything is the command to run inside it rather than an option to `wsl.exe`.
const RUN: (&str, &str, &str) = ("wsl.exe", "-d", "--");

/// The shell the question is handed to inside the distribution.
///
/// `sh` rather than the account's own shell, deliberately: the question is *about* that
/// shell, so running it in that shell would need to know the answer first. Every
/// distribution has an `sh`, and the question is written in the subset all of them share.
const POSIX_SHELL: &str = "sh";

/// How long a distribution has to say what shell it runs before the session starts without
/// its answer.
///
/// **Four times the SSH probe's three seconds, and the difference is what is being waited
/// for.** B9 asks over a connection that has already carried a key exchange and an
/// authentication, so the far end is awake and one line is all that is left. This call can
/// be the one that *boots* the distribution's virtual machine, and that is not a network
/// wait at all.
///
/// **The number comes from measurement, and the first number tried was wrong.** On the
/// developer's machine on 2026-08-29, Ubuntu 24.04 under WSL 2.5.7.0, `wsl.exe -- sh -c` was
/// timed warm and then cold, four times, with `wsl --shutdown` between the cold runs. Warm:
/// 148, 152, 165, 181, 188, 206 milliseconds. Cold: 5.35, 5.49, 5.74 and 6.30 **seconds** —
/// a spread a six-second deadline lands in the middle of, which is the worst place for one
/// to be, because it makes a cold bash distribution a coin toss between being integrated and
/// being unnamed. Twelve is roughly twice the slowest cold start seen, which leaves the
/// deadline for what it is meant to catch: a distribution that is not coming up at all.
///
/// **`wsl.exe -l -q` does not warm anything**, measured at 57 to 74 milliseconds whether the
/// machine was cold or warm. It is the first command run *inside* a distribution that boots
/// it, which is why the connect list can be built cheaply and this cannot.
///
/// **It is not twelve seconds added to the time before a prompt, and on a cold machine it is
/// barely added at all.** Warm — the ordinary case, and every case after the first — a
/// distribution answers in under a fifth of a second. Cold, this call is *what does the
/// booting*, so the session's own start then finds a distribution that is already up; the
/// boot was going to be paid either way, and roadmap 23.10 is the entry about a listener
/// being told so while it happens.
const PATIENCE: Duration = Duration::from_secs(12);

/// How often the deadline is checked while the answer is outstanding.
///
/// Short enough that a warm distribution is not held back by the polling, long enough that
/// waiting costs a thread almost nothing.
const TICK: Duration = Duration::from_millis(25);

/// The directory a Store package's files live in, which is what says an install came from the
/// Store rather than from an installer.
const WINDOWS_APPS: &str = "WindowsApps";

/// The separator between a package full name and its publisher id:
/// `Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe`.
const PUBLISHER: &str = "__";

/// The directory every versioned PowerShell 7 install sits under, in `%ProgramFiles%` and its
/// twins — and the one Windows Terminal reads the version from, because the file cannot be
/// trusted to say (decision 3).
const POWERSHELL: &str = "PowerShell";

/// This machine, asked directly.
///
/// Holds nothing and caches nothing: a user who installs a distribution while Acter is
/// open should see it in the next list they open, and a cache would make them restart the
/// program to be told the truth.
#[derive(Debug, Default)]
pub struct ThisMachine;

impl ThisMachine {
    pub fn new() -> Self {
        Self
    }
}

impl InstalledShells for ThisMachine {
    /// **Three failures told apart, because they need three different sentences** (spec
    /// B5.3, decision 6). `wsl.exe` that will not start at all is a machine without the
    /// feature; `wsl.exe` that runs and refuses is a machine whose WSL is broken, and it
    /// is read back WSL's own explanation; `wsl.exe` that runs, succeeds and names nothing
    /// is a machine with WSL and no distribution in it.
    ///
    /// **The refusal is read from standard output**, which is where `wsl.exe` writes it —
    /// measured on 2026-08-24, where an unknown distribution produced
    /// `There is no distribution with the supplied name.` on stdout, exit code 127, and an
    /// empty standard error. Reading only stderr would have produced a broken-WSL sentence
    /// with nothing after it.
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

    /// Every file this machine would start for this name, most preferred first.
    ///
    /// **Looked up rather than run**, which is the whole point and which B5.3 decided: the
    /// question is asked while building a list of things the user *may* connect to, and
    /// starting each candidate to find out whether it starts would open sessions nobody
    /// asked for. Nothing here reads a version out of a file either (decision 3).
    fn installs(&self, program: &str) -> Vec<ShellInstall> {
        let mut found: Vec<ShellInstall> = Vec::new();
        // **`PATH` first, and every match rather than the first**, because the order is the
        // answer: the entry `PATH` resolves first is what typing the name in any other
        // terminal starts, and that is the one fact no other source here knows.
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
                    // The registry is the only source that says a version out loud, and for
                    // Windows PowerShell it is the right one — so it wins over what the path
                    // would have guessed, and only where the path guessed nothing.
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

    /// Which shell this distribution's account is configured to run.
    ///
    /// **A second `wsl.exe`, never a line typed into the session** (spec B5.5, decision 1).
    /// This is the cheap half of B9's decision 7: WSL needs no channel and no protocol, only
    /// another invocation whose output the terminal buffer never sees. Typing the question
    /// into the session instead would put a command nobody typed in front of a screen
    /// reader, which is B4.9's whole subject.
    ///
    /// **Advisory, never a gate** (decision 3). A client that is not there, a distribution
    /// that will not start, an answer that is not a shell name and a deadline that passed
    /// are all the same `None`, and `None` costs the session nothing: it starts anyway,
    /// unintegrated and unnamed.
    ///
    /// **Standard error is discarded on purpose**, which is the opposite of what
    /// [`wsl_distributions`](Self::wsl_distributions) does with it. There, a refusal is read
    /// back to the user in WSL's own words because they asked to see a list and are owed an
    /// explanation. Here nobody asked a question: a complaint would be an interruption in
    /// the seconds before a prompt, describing an internal probe the user never started.
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

/// What a command wrote to standard output, or `None` if it did not finish in time.
///
/// **The child is killed rather than abandoned.** A `wsl.exe` left running after its answer
/// stopped being wanted would keep a distribution awake and hold a pipe open, and the caller
/// has already moved on to starting the session.
///
/// **Standard output is drained on its own thread**, which is what makes the deadline a
/// deadline rather than a suggestion: a child that fills the pipe buffer blocks on the write
/// and never exits, so a parent waiting for exit before reading would wait forever on
/// exactly the output that was too long. Standard input is closed for the mirror-image
/// reason — a child that asks a question nobody is there to answer would block on the read.
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
            // Whether it succeeded is not asked. A distribution that answered and then
            // exited non-zero — `getent` finding nothing, the fallback printing an empty
            // `$SHELL` — still wrote whatever it wrote, and the reading is what decides
            // whether that is a shell name. One judgement, in one place.
            Ok(Some(_)) => return reading.join().ok(),
            Ok(None) if Instant::now() < deadline => thread::sleep(TICK),
            // The deadline passed, or the child could not be waited on at all. The reading
            // thread ends on its own when the kill closes the pipe.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Every file `PATH` resolves this name to, in the order Windows would consider them.
///
/// A name with no extension is looked for under every `PATHEXT` suffix, and a name that is a
/// location is not looked for on `PATH` at all — both of which are the platform's own rules,
/// and both of which `which_all` applies. It replaced a hand-written walk in B5.7: the
/// hand-written one answered only the *first* match, which is the half of this question that
/// cannot see a second install.
fn on_path(program: &str) -> Vec<PathBuf> {
    which_all(program)
        .map(|found| found.collect())
        .unwrap_or_default()
}

/// One candidate, resolved through whatever stands between the name and the file.
///
/// **The alias is resolved here rather than left for verification** (decision 4). An app
/// execution alias can be started and cannot be read, so an install that stayed pointed at
/// one would have nothing to check; resolving it to the package file it stands for gives
/// something with a real signature. A resolution that fails leaves the alias in place, which
/// verification then reports as unverifiable with its reason — never quietly trusted.
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

/// The file an execution alias stands for, or `None` for anything that is not one — and for
/// an alias whose package file is not there after all.
#[cfg(windows)]
fn package(candidate: &Path) -> Option<PathBuf> {
    crate::signature_target(candidate)
        .map(|link| link.program)
        .filter(|program| program.is_file())
}

/// No aliases anywhere else, so nothing to resolve.
#[cfg(not(windows))]
fn package(_candidate: &Path) -> Option<PathBuf> {
    None
}

/// Adds an install unless the same file is already there.
///
/// **Deduplicated by the file, not by how it was found** (decision 2), and the first one
/// wins — which is `PATH`'s, so the default survives being found again by a known root. On
/// the developer's machine this is what turns the Store package and the execution alias
/// beside it into the one install they are.
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

/// Whether two paths name the same file.
///
/// Compared case-insensitively because Windows filenames are, and after canonicalising when
/// that is possible — a canonicalise that fails is not a reason to list one file twice, so
/// the raw paths are compared instead.
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

/// Where a file came from, read off the path and nothing else.
///
/// **Decision 3, made mechanical.** The provenance is the versioned directory name, the Store
/// package identity, or nothing at all — never the file's own version resource, which
/// measured 2026-08-27 reports the *Windows build* for `powershell.exe` rather than 5.1.
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
        return Provenance::Windows;
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

/// The package family a package full name belongs to:
/// `Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe` is the `Microsoft.PowerShell_8wekyb3d8bbwe`
/// family, which is the identity `CreateProcess` resolves an alias through and the identity
/// two versions of one package share.
fn family(full_name: &str) -> Option<String> {
    let (before, publisher) = full_name.split_once(PUBLISHER)?;
    let name = before.split('_').next()?;
    if name.is_empty() || publisher.is_empty() {
        return None;
    }
    Some(format!("{name}_{publisher}"))
}

/// Whether this file is one Windows ships, which is the directory it is in and nothing else.
///
/// `WindowsPowerShell\v1.0` is under `System32`, so both editions Windows ships answer yes —
/// which is right: neither can be uninstalled and neither is told from another install of
/// itself.
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

/// The directory `at` components into this path, for asking whether the file sits directly in
/// a versioned install directory rather than somewhere below one.
fn root_of(program: &Path, at: usize) -> PathBuf {
    program.components().take(at + 1).collect()
}

/// What `wsl.exe` said when it refused, as one speakable sentence.
///
/// Falls back to a sentence of our own rather than to an empty string: a message that ends
/// mid-air after "it could not list its distributions." leaves a listener waiting for the
/// half that never comes.
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

/// Every environment variable, for the roots that only exist on some machines — read here so
/// [`roots`] takes them rather than reaching for them.
fn environment() -> Vec<(String, PathBuf)> {
    vars_os()
        .filter_map(|(name, value)| Some((name.to_str()?.to_uppercase(), PathBuf::from(value))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The measured shape of a Store install** (2026-08-27): `pwsh` resolves to a package
    /// directory under `WindowsApps`, and what tells it from another install is the package
    /// family rather than anything in the file.
    #[test]
    fn a_file_under_a_store_package_is_named_by_its_package_family() {
        let provenance = provenance(Path::new(
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe",
        ));

        assert_eq!(
            provenance,
            Provenance::Store {
                family: "Microsoft.PowerShell_8wekyb3d8bbwe".to_owned(),
                preview: false,
            }
        );
    }

    /// The preview package is the same family shape with preview in the name, and it has to
    /// read differently or two entries would sound identical.
    #[test]
    fn a_preview_package_says_so() {
        let provenance = provenance(Path::new(
            r"C:\Program Files\WindowsApps\Microsoft.PowerShellPreview_7.7.0.0_x64__8wekyb3d8bbwe\pwsh.exe",
        ));

        assert_eq!(
            provenance,
            Provenance::Store {
                family: "Microsoft.PowerShellPreview_8wekyb3d8bbwe".to_owned(),
                preview: true,
            }
        );
    }

    /// **The version comes from the directory** (decision 3), which is where Windows Terminal
    /// reads it from too — and never from the file, which measured 2026-08-27 reports the
    /// Windows build rather than its own version.
    #[test]
    fn an_msi_install_is_named_by_the_directory_it_was_installed_into() {
        assert_eq!(
            provenance(Path::new(r"C:\Program Files\PowerShell\7\pwsh.exe")),
            Provenance::Directory {
                version: "7".to_owned(),
                preview: false,
            }
        );
        assert_eq!(
            provenance(Path::new(r"C:\Program Files\PowerShell\7-preview\pwsh.exe")),
            Provenance::Directory {
                version: "7-preview".to_owned(),
                preview: true,
            }
        );
    }

    /// **A file somewhere that says nothing is indeterminable, not guessed at** (decision 3).
    /// A dotnet tool, a scoop shim and a directory somebody put on `PATH` are all this.
    #[test]
    fn a_file_somewhere_that_says_nothing_is_reported_as_saying_nothing() {
        for anywhere in [
            r"C:\Users\someone\.dotnet\tools\pwsh.exe",
            r"C:\Users\someone\scoop\shims\pwsh.exe",
            r"C:\tools\pwsh\pwsh.exe",
            // Under the PowerShell directory but not *in* a versioned one, so the name beside
            // it is not a version and must not be read as one.
            r"C:\Program Files\PowerShell\7\Modules\Something\pwsh.exe",
        ] {
            assert_eq!(
                provenance(Path::new(anywhere)),
                Provenance::Indeterminable,
                "{anywhere} says nothing about itself"
            );
        }
    }

    /// The shells Windows ships are Windows', both of them — `WindowsPowerShell\v1.0` is
    /// under `System32`, and neither can be told from another install of itself because
    /// neither can be uninstalled.
    #[cfg(windows)]
    #[test]
    fn the_shells_windows_ships_come_from_windows() {
        let system = var_os("SystemRoot").expect("Windows always sets this");
        let system = PathBuf::from(system);

        assert_eq!(
            provenance(&system.join("system32").join("cmd.exe")),
            Provenance::Windows
        );
        assert_eq!(
            provenance(
                &system
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            ),
            Provenance::Windows,
            "case is not what tells a directory apart on this platform"
        );
    }

    /// A package full name that is not one resolves to no family, rather than to a family
    /// made out of whatever was there — which would name an install after a directory
    /// somebody happened to call `WindowsApps`.
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

    /// **The lookup and the spawn have to agree about what "installed" means**, which is now
    /// literal: this asks the machine about the shell every Windows install has, and what
    /// comes back is the file that will be started. No process is started either way.
    #[cfg(windows)]
    #[test]
    fn the_shell_every_windows_machine_has_resolves_to_a_file_that_is_there() {
        let machine = ThisMachine::new();

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
            assert_eq!(first.provenance, Provenance::Windows);
        }
        assert!(
            machine
                .installs("acter-no-such-program-exists.exe")
                .is_empty(),
            "and a name nothing resolves has no installs at all"
        );
    }

    /// One file is one install however many sources found it — which on the developer's
    /// machine is what turns the Store package directory and the execution alias beside it
    /// into the single install they are.
    #[cfg(windows)]
    #[test]
    fn one_file_found_twice_is_listed_once() {
        let installs = ThisMachine::new().installs("cmd.exe");

        let mut seen: Vec<String> = installs
            .iter()
            .map(|install| install.program.to_string_lossy().to_lowercase())
            .collect();
        seen.sort();
        let listed = seen.len();
        seen.dedup();

        assert_eq!(seen.len(), listed, "no file appears twice: {installs:?}");
    }

    /// A path is a location and not a name, so `PATH` is not searched for it — the same rule
    /// Windows applies, and the difference between checking one file and checking one per
    /// directory in the environment.
    #[cfg(windows)]
    #[test]
    fn a_program_named_by_its_full_path_is_the_only_install_of_itself() {
        let cmd = PathBuf::from(var_os("SystemRoot").expect("Windows sets this"))
            .join("system32")
            .join("cmd.exe");

        let installs = ThisMachine::new().installs(&cmd.to_string_lossy());

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

    /// The sentence a listener actually hears when WSL refuses. It is WSL's own words,
    /// decoded from the UTF-16LE they arrive in, on the stream WSL really writes them to.
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

    /// Nothing on either stream still ends in a whole sentence, because the reason is
    /// appended to one and a listener would otherwise be left waiting for it.
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
