//! Adapter: what this computer actually has, behind acter-core's `InstalledShells` port —
//! the WSL distributions `wsl.exe` reports, and whether a named program exists to be
//! started.
//!
//! **The only module in this crate that touches the world.** Everything else here is
//! knowledge: what `cmd.exe` is started with is the same answer on every Windows machine,
//! and which distributions are installed is the same answer on no two machines at all.
//! That difference is ARCHITECTURE's classifying question, and it is why this is the one
//! file with a `Command` in it.
//!
//! **A workspace test run must never reach this module.** `cargo test --workspace` spawns
//! no process, which is why the decode it depends on lives in
//! [`distributions`](crate::wsl::distributions) as a pure function over captured bytes and
//! is tested there, while the only tests here are the ones that need no process at all.
//! The live path is exercised by an `#[ignore]`d suite in `acter-transports`.

use std::env::{split_paths, var_os};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use acter_core::{InstalledShells, NoDistributions};

use crate::wsl::distributions::{decode_utf16le, distributions};

/// The client asked what is installed, and the flags that make it answer with names and
/// nothing else.
///
/// `-l -q` rather than `--list --quiet` for no reason except that this is what was
/// measured; `-q` is what strips the header line and the `(Default)` suffix, leaving one
/// bare name per line.
const LIST: (&str, [&str; 2]) = ("wsl.exe", ["-l", "-q"]);

/// The extensions Windows will append to a program name that has none, when `PATHEXT`
/// itself cannot be read. Windows' own documented default, restated here so a machine with
/// an unusual environment still finds `wsl.exe` when the user typed `wsl`.
const PATHEXT_FALLBACK: &str = ".COM;.EXE;.BAT;.CMD";

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

    /// Whether a program of this name exists somewhere Windows would start it from.
    ///
    /// **Looked up rather than run**, which is the whole point: the question is asked
    /// while building a list of things the user *may* connect to, and starting each
    /// candidate to find out whether it starts would open sessions nobody asked for.
    fn is_available(&self, program: &str) -> bool {
        candidates(program, path(), pathext())
            .iter()
            .any(|candidate| candidate.is_file())
    }
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

/// Every file Windows would consider when asked to start `program`, in the order it would
/// consider them.
///
/// Separated from the filesystem check so the rule can be tested without creating files:
/// what is worth pinning here is *which* paths are looked at, and whether one of them
/// happens to exist is the machine's answer rather than this function's.
fn candidates(program: &str, path: Vec<PathBuf>, pathext: String) -> Vec<PathBuf> {
    let named = Path::new(program);
    // A program with a separator in it is a location, not a name, so `PATH` is not
    // consulted for it at all — the same rule Windows applies.
    let directories = if named.components().count() > 1 {
        vec![PathBuf::new()]
    } else {
        path
    };
    let mut candidates = Vec::new();
    for directory in directories {
        candidates.push(directory.join(named));
        if named.extension().is_none() {
            for extension in pathext.split(';').filter(|piece| !piece.is_empty()) {
                let with_extension = format!("{program}{extension}");
                candidates.push(directory.join(with_extension));
            }
        }
    }
    candidates
}

fn path() -> Vec<PathBuf> {
    var_os("PATH")
        .map(|value| split_paths(&value).collect())
        .unwrap_or_default()
}

fn pathext() -> String {
    var_os("PATHEXT")
        .as_deref()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| PATHEXT_FALLBACK.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_path() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Windows\system32"),
            PathBuf::from(r"C:\bin"),
        ]
    }

    /// A name with no extension is the way a person says a program, and Windows is the
    /// only platform where that name is not the filename. Without this, a profile saying
    /// `wsl` would be reported as not installed on a machine that has it.
    ///
    /// The suffixes keep `PATHEXT`'s own casing — the real variable is upper case — which
    /// costs nothing, since the filesystem this is checked against is case-insensitive.
    #[test]
    fn a_name_with_no_extension_is_looked_for_under_every_pathext_suffix() {
        let looked_at = candidates("wsl", search_path(), ".COM;.EXE".to_owned());

        assert!(looked_at.contains(&PathBuf::from(r"C:\Windows\system32\wsl.EXE")));
        assert!(looked_at.contains(&PathBuf::from(r"C:\Windows\system32\wsl.COM")));
        assert!(looked_at.contains(&PathBuf::from(r"C:\bin\wsl.EXE")));
    }

    /// The lookup and the spawn have to agree about what "available" means, so this asks
    /// the machine about a program every Windows install has and about one no install has.
    /// No process is started either way — that is the point of looking a name up rather
    /// than running it.
    #[test]
    fn a_program_every_windows_machine_has_is_available_and_an_invented_one_is_not() {
        let machine = ThisMachine::new();

        assert!(
            machine.is_available("cmd.exe"),
            "cmd.exe is on every machine"
        );
        assert!(
            machine.is_available("cmd"),
            "and is found without its extension"
        );
        assert!(!machine.is_available("acter-no-such-program-exists.exe"));
    }

    /// A name that already has one is not decorated with another: looking for
    /// `wsl.exe.EXE` is how a present program gets reported as missing.
    #[test]
    fn a_name_that_already_has_an_extension_is_looked_for_exactly_as_it_stands() {
        let looked_at = candidates("wsl.exe", search_path(), ".COM;.EXE".to_owned());

        assert_eq!(
            looked_at,
            [
                PathBuf::from(r"C:\Windows\system32\wsl.exe"),
                PathBuf::from(r"C:\bin\wsl.exe"),
            ]
        );
    }

    /// A path is a location and not a name, so `PATH` is not searched for it — the same
    /// rule Windows applies, and the difference between checking one file and checking one
    /// per directory in the environment.
    #[test]
    fn a_program_named_by_its_full_path_is_not_looked_for_anywhere_else() {
        let looked_at = candidates(r"C:\tools\pwsh.exe", search_path(), ".EXE".to_owned());

        assert_eq!(looked_at, [PathBuf::from(r"C:\tools\pwsh.exe")]);
    }

    /// The order is the order Windows would use, because "available" has to mean the same
    /// thing here as it does when the transport actually spawns the program.
    #[test]
    fn the_directories_are_looked_at_in_the_order_the_environment_lists_them() {
        let looked_at = candidates("wsl.exe", search_path(), String::new());

        assert_eq!(
            looked_at.first(),
            Some(&PathBuf::from(r"C:\Windows\system32\wsl.exe"))
        );
    }

    /// An empty `PATH` is not a crash and not a panic: it is a machine where nothing is
    /// available, which is a thing the connect list can say.
    #[test]
    fn a_machine_with_no_search_path_offers_nothing_rather_than_failing() {
        assert!(candidates("wsl", Vec::new(), ".EXE".to_owned()).is_empty());
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
