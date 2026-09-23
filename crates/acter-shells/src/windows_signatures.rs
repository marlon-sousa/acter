//! Adapter: who signed the files this machine would start, behind acter-core's `Signatures`
//! port.

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

#[derive(Debug, Default)]
pub struct WindowsTrust {
    remembered: Mutex<HashMap<PathBuf, (Stamp, Verdict)>>,
}

/// A file replaced under the same path is checked again, but the cache is no security boundary:
/// nothing closes the gap between a check and a spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

impl WindowsTrust {
    pub fn new() -> Self {
        Self::default()
    }

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

    fn known(&self, program: &Path) -> Option<Verdict> {
        self.recall(program, stamp(program))
    }
}

/// `None` for a file that cannot be looked at, whose verdict is then never remembered.
fn stamp(program: &Path) -> Option<Stamp> {
    let about = std::fs::metadata(program).ok()?;
    Some(Stamp {
        size: about.len(),
        modified: about.modified().ok(),
    })
}

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

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("acter-b5.7-{name}"))
    }

    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
            .join("system32")
            .join("cmd.exe")
    }

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

    #[test]
    fn a_program_nothing_has_signed_is_untrusted_and_says_which_kind_of_untrusted() {
        let copied = scratch("unsigned.exe");
        let mut bytes = read(cmd()).expect("cmd.exe can be read");
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

    #[test]
    fn a_file_that_cannot_be_read_is_unverifiable_rather_than_unsigned() {
        let verdict = WindowsTrust::new().verdict(&scratch("no-such-file.exe"));

        let Verdict::Unverifiable { why } = verdict else {
            panic!("a file nobody could look at is neither trusted nor accused");
        };
        assert!(why.ends_with('.'), "it is read aloud, so it ends: {why}");
    }

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

    #[test]
    fn nothing_is_known_about_a_file_nobody_has_checked() {
        assert_eq!(WindowsTrust::new().known(&cmd()), None);
    }
}
