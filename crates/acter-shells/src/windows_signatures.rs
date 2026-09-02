//! Adapter: who signed the files this machine would start, behind acter-core's `Signatures`
//! port.
//!
//! **The one place in this product where being wrong is a security answer**, which is why
//! spec B5.7 decision 5 chose `windows-sys` over a third-party wrapper: the declarations are
//! Microsoft's, already a dependency of `acter-transports`, and a small low-traffic crate is
//! a poor thing to put here.
//!
//! Three conversations with Windows, in three files, because they are three different
//! questions: [`trust`] asks whether this machine trusts a signature and whose it is,
//! [`alias`] asks what an app execution alias really points at, and this file is the port —
//! what is remembered, and for how long.
//!
//! **What is remembered, and why by more than the path** (decision 7). The verdict is cached
//! by resolved path, size and last-write time for the life of the process, so the second
//! connection to the same shell costs nothing and a file that was *replaced* between two
//! connections is checked again rather than vouched for by what its predecessor was. It is
//! not a security boundary — nothing on Windows closes the window between a check and a
//! spawn (decision 1) — it is what stops the check being paid for twice for no reason.
//!
//! **A workspace test run reaches this module, and that is deliberate.** Unlike
//! [`windows_machine`](crate::windows_machine), verifying a signature spawns no process and opens no
//! session: it reads a file, this machine's catalogs and this machine's certificate stores,
//! and with `WTD_CACHE_ONLY_URL_RETRIEVAL` it reaches no network. So the tests here can ask
//! the real Windows about real files — including the one every Windows machine has.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use acter_core::{Signatures, Verdict};

mod alias;
mod trust;

pub(crate) use alias::target;

/// This machine's own answer about a file, remembered for the life of the process.
#[derive(Debug, Default)]
pub struct WindowsTrust {
    remembered: Mutex<HashMap<PathBuf, (Stamp, Verdict)>>,
}

/// What makes a remembered verdict still about the file it was made about.
///
/// **Size and last-write time rather than the path alone.** The path is the *name* of a
/// file, and the thing this check defeats is somebody putting a different file where a name
/// used to point — so a cache keyed on the name would be the same mistake one layer up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

impl WindowsTrust {
    pub fn new() -> Self {
        Self::default()
    }

    /// The verdict already paid for about *this* file, or `None`.
    fn recall(&self, program: &Path, stamp: Option<Stamp>) -> Option<Verdict> {
        let stamp = stamp?;
        let remembered = self.remembered.lock().expect("signature cache poisoned");
        remembered
            .get(program)
            .filter(|(when, _)| *when == stamp)
            .map(|(_, verdict)| verdict.clone())
    }
}

impl Signatures for WindowsTrust {
    fn verdict(&self, program: &Path) -> Verdict {
        let stamp = stamp(program);
        if let Some(remembered) = self.recall(program, stamp) {
            return remembered;
        }
        let verdict = trust::verify(program);
        if let Some(stamp) = stamp {
            self.remembered
                .lock()
                .expect("signature cache poisoned")
                .insert(program.to_path_buf(), (stamp, verdict.clone()));
        }
        verdict
    }

    /// **Nothing is verified here** (decision 7), which is what lets the connect list name a
    /// verdict without paying for one. Reading the file's size and time is a `stat` and
    /// reaches no catalog, no certificate store and no network.
    fn known(&self, program: &Path) -> Option<Verdict> {
        self.recall(program, stamp(program))
    }
}

/// The file as it stands right now, or `None` for one that cannot even be looked at — which
/// is the app execution alias, and which is therefore never remembered.
fn stamp(program: &Path) -> Option<Stamp> {
    let about = std::fs::metadata(program).ok()?;
    Some(Stamp {
        size: about.len(),
        modified: about.modified().ok(),
    })
}

/// A path as the null-terminated wide string every call in this module takes.
///
/// Here rather than in either submodule because both need it and it is the same three lines
/// either way — a shared spelling of one Windows convention, not a drawer.
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::fs::{File, read, write};
    use std::io::Write;

    use acter_core::{Fault, Signer, ThisComputer};

    use crate::WindowsMachine;

    use super::*;

    /// Where a test may write, from the environment the runner sets.
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("acter-b5.7-{name}"))
    }

    /// The shell every Windows machine has and cannot remove.
    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
            .join("system32")
            .join("cmd.exe")
    }

    /// **The acceptance criterion, asked of the real Windows** (spec B5.7, definition of
    /// done): the shell Windows ships verifies as trusted and signed by Microsoft — and it
    /// does so through the *catalog*, which is the measurement the whole design turns on. A
    /// verification built the obvious way would report this file as unsigned.
    #[test]
    fn the_shell_windows_ships_is_trusted_and_signed_by_microsoft() {
        let verdict = WindowsTrust::new().verdict(&cmd());

        assert_eq!(
            verdict,
            Verdict::Trusted {
                signer: Signer::Microsoft
            },
            "cmd.exe is catalog-signed by Microsoft on every Windows machine"
        );
    }

    /// **The other half of the acceptance criterion** (definition of done): PowerShell 7
    /// verifies as trusted and signed by Microsoft, whether it was installed by the MSI or
    /// reached through the Store's execution alias — which is the case this machine has, and
    /// the case that cannot be verified at all without resolving the alias first.
    ///
    /// **Conditional, and it says so rather than passing quietly.** A machine with no
    /// PowerShell 7 is a supported machine, and this is the one assertion here that depends
    /// on what is installed; every other test in this file builds what it needs.
    #[test]
    fn powershell_seven_is_trusted_and_signed_by_microsoft_wherever_it_came_from() {
        let installs = WindowsMachine::new().installs("pwsh.exe");
        if installs.is_empty() {
            eprintln!("no PowerShell 7 on this machine, so there is nothing to verify");
            return;
        }
        let signatures = WindowsTrust::new();

        for install in installs {
            assert_eq!(
                signatures.verdict(&install.program),
                Verdict::Trusted {
                    signer: Signer::Microsoft
                },
                "{install:?}"
            );
        }
    }

    /// **A real unsigned executable, made by the test rather than found on the machine.**
    ///
    /// A copy of `cmd.exe` still verifies — catalogs claim a file by its *hash*, not by where
    /// it is — so the copy is changed by one byte, which takes it out of every catalog on
    /// this machine and leaves a genuine PE that nothing has signed. That is the fixture the
    /// definition of done asks for, built from something every Windows machine has.
    #[test]
    fn a_program_nothing_has_signed_is_untrusted_and_says_which_kind_of_untrusted() {
        let copied = scratch("unsigned.exe");
        let mut bytes = read(cmd()).expect("cmd.exe can be read");
        // Somewhere well inside the file, so what changes is content rather than a header
        // Windows would refuse to parse at all.
        let at = bytes.len() / 2;
        bytes[at] ^= 0xff;
        write(&copied, &bytes).expect("the scratch directory is writable");

        let verdict = WindowsTrust::new().verdict(&copied);

        assert_eq!(
            verdict,
            Verdict::Untrusted {
                fault: Fault::NotSigned
            },
            "a real executable that no catalog claims and that carries no signature"
        );
        assert!(
            !verdict.settled(),
            "so the connection asks before starting it"
        );
        let _ = std::fs::remove_file(&copied);
    }

    /// And the unchanged copy is trusted wherever it is, which is what makes the byte flip
    /// above the thing being tested rather than the directory.
    #[test]
    fn a_catalog_claims_a_file_by_its_hash_rather_than_by_where_it_is() {
        let copied = scratch("copy.exe");
        write(&copied, read(cmd()).expect("cmd.exe can be read")).expect("scratch is writable");

        assert_eq!(
            WindowsTrust::new().verdict(&copied),
            Verdict::Trusted {
                signer: Signer::Microsoft
            }
        );
        let _ = std::fs::remove_file(&copied);
    }

    /// **Never quietly condemned** (decision 4). A file that is not there is unverifiable
    /// with its reason, and specifically not "nothing signed it" — which would be an
    /// accusation about a file nobody looked at.
    #[test]
    fn a_file_that_cannot_be_read_is_unverifiable_rather_than_unsigned() {
        let verdict = WindowsTrust::new().verdict(&scratch("no-such-file.exe"));

        let Verdict::Unverifiable { why } = verdict else {
            panic!("a file nobody could look at is neither trusted nor accused");
        };
        assert!(why.ends_with('.'), "it is read aloud, so it ends: {why}");
    }

    /// **Decision 7's cache, and the half that matters.** The same file answers from what was
    /// already paid for; a file *replaced* under the same name is checked again, because a
    /// cache keyed on the name would be the very substitution this check exists to catch.
    #[test]
    fn a_replaced_file_is_checked_again_rather_than_vouched_for_by_its_predecessor() {
        let path = scratch("replaced.exe");
        write(&path, b"not a program at all").expect("scratch is writable");
        let signatures = WindowsTrust::new();

        let first = signatures.verdict(&path);
        assert_eq!(
            signatures.known(&path),
            Some(first),
            "the same file answers from what was already paid for"
        );

        let mut grown = File::create(&path).expect("scratch is writable");
        grown
            .write_all(&read(cmd()).expect("cmd.exe can be read"))
            .expect("scratch is writable");
        drop(grown);

        assert_eq!(
            signatures.known(&path),
            None,
            "a different file under the same name is not what was checked"
        );
        assert_eq!(
            signatures.verdict(&path),
            Verdict::Trusted {
                signer: Signer::Microsoft
            },
            "and checking it again answers about the file that is there now"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **Decision 7 from the list's side**: nothing has been paid for, so nothing is known,
    /// and asking costs no catalog lookup and no network.
    #[test]
    fn nothing_is_known_about_a_file_nobody_has_checked() {
        assert_eq!(WindowsTrust::new().known(&cmd()), None);
    }
}
