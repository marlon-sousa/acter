//! Adapter: who signed the files a Mac would start, behind acter-core's `Signatures` port.
//!
//! On macOS 15 all seven shells in `/etc/shells` satisfy `anchor apple`, with the leaf
//! certificate "Software Signing".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::SystemTime;

use acter_core::{Fault, Signatures, Signer, Verdict};
use objc2_core_foundation::{CFArray, CFDictionary, CFRetained, CFString, CFURL, CFURLPathStyle};
use objc2_security::{
    SecCSFlags, SecCertificate, SecCode, SecRequirement, SecStaticCode, errSecCSSignatureFailed,
    errSecCSUnsigned, kSecCSSigningInformation, kSecCodeInfoCertificates,
};

/// Satisfied by the binaries Apple ships and nothing else.
const ANCHOR_APPLE: &str = "anchor apple";

/// Satisfied by a Developer ID or Mac App Store signature.
const ANCHOR_APPLE_GENERIC: &str = "anchor apple generic";

#[derive(Debug, Default)]
pub struct AppleTrust {
    remembered: Mutex<HashMap<PathBuf, (Stamp, Verdict)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

impl AppleTrust {
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

impl Signatures for AppleTrust {
    fn verdict(&self, program: &Path) -> Verdict {
        let stamp = stamp(program);
        if let Some(remembered) = self.recall(program, stamp) {
            return remembered;
        }
        let verdict = verify(program);
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

fn verify(program: &Path) -> Verdict {
    let Some(code) = static_code(program) else {
        return Verdict::Unverifiable {
            why: "macOS could not read a signature from this file at all.".to_owned(),
        };
    };
    if let Some(refused) = intact(&code) {
        return refused;
    }
    if satisfies(&code, ANCHOR_APPLE) {
        return Verdict::Trusted {
            signer: Signer::Apple,
        };
    }
    match (satisfies(&code, ANCHOR_APPLE_GENERIC), leaf_name(&code)) {
        (true, Some(name)) => Verdict::Trusted {
            signer: Signer::Other { name },
        },
        (true, None) => Verdict::Unverifiable {
            why: "macOS trusts this file's signature but could not say who holds it.".to_owned(),
        },
        (false, signer @ Some(_)) => Verdict::Untrusted {
            fault: Fault::UntrustedRoot { signer },
        },
        (false, None) => Verdict::Untrusted {
            fault: Fault::AdHoc,
        },
    }
}

fn static_code(program: &Path) -> Option<CFRetained<SecStaticCode>> {
    let path = CFString::from_str(&program.to_string_lossy());
    let url = CFURL::with_file_system_path(
        None,
        Some(&path),
        CFURLPathStyle::CFURLPOSIXPathStyle,
        false,
    )?;
    let mut code: *const SecStaticCode = std::ptr::null();
    // SAFETY: `url` outlives the call, and `code` is a pointer this stack frame owns.
    // `Create` calls take default flags: given the checking flags, the framework returns nothing.
    let status = unsafe {
        SecStaticCode::create_with_path(&url, SecCSFlags::DefaultFlags, NonNull::from(&mut code))
    };
    if status != 0 {
        return None;
    }
    // SAFETY: a non-null pointer from a `Create` function is an owned reference, which is
    // what `from_raw` takes responsibility for releasing.
    NonNull::new(code.cast_mut()).map(|code| unsafe { CFRetained::from_raw(code) })
}

fn intact(code: &SecStaticCode) -> Option<Verdict> {
    // SAFETY: `code` is a live object and no requirement is passed, which is the documented
    // way to ask whether the signature alone holds.
    let status = unsafe { SecStaticCode::check_validity(code, flags(), None) };
    if status == 0 {
        return None;
    }
    if status == errSecCSUnsigned {
        return Some(Verdict::Untrusted {
            fault: Fault::NotSigned,
        });
    }
    if status == errSecCSSignatureFailed {
        return Some(Verdict::Untrusted {
            fault: Fault::Tampered,
        });
    }
    Some(Verdict::Unverifiable {
        why: "macOS refused this file's signature without saying which way.".to_owned(),
    })
}

fn satisfies(code: &SecStaticCode, requirement: &str) -> bool {
    let text = CFString::from_str(requirement);
    let mut parsed: *mut SecRequirement = std::ptr::null_mut();
    // SAFETY: `text` outlives the call and `parsed` is owned by this frame.
    let made = unsafe {
        SecRequirement::create_with_string(
            &text,
            SecCSFlags::DefaultFlags,
            NonNull::from(&mut parsed),
        )
    };
    let Some(parsed) = NonNull::new(parsed) else {
        return false;
    };
    // SAFETY: a non-null pointer from a `Create` function is an owned reference.
    let parsed = unsafe { CFRetained::from_raw(parsed) };
    if made != 0 {
        return false;
    }
    // SAFETY: both objects are live for the call.
    unsafe { SecStaticCode::check_validity(code, flags(), Some(&parsed)) == 0 }
}

/// `None` for an ad-hoc signature, which carries no certificates.
fn leaf_name(code: &SecStaticCode) -> Option<String> {
    let mut information: *const CFDictionary = std::ptr::null();
    // SAFETY: `code` is live and `information` is owned by this frame.
    let status = unsafe {
        SecCode::copy_signing_information(
            code,
            SecCSFlags(kSecCSSigningInformation),
            NonNull::from(&mut information),
        )
    };
    let information = NonNull::new(information.cast_mut())?;
    // SAFETY: a non-null pointer from a `Copy` function is an owned reference.
    let information = unsafe { CFRetained::from_raw(information) };
    if status != 0 {
        return None;
    }
    let key: *const CFString = unsafe { kSecCodeInfoCertificates };
    // SAFETY: the dictionary is live, and the key is the framework's own constant.
    let certificates = unsafe { CFDictionary::value(&information, key.cast()) };
    if certificates.is_null() {
        return None;
    }
    // SAFETY: this key's value is documented as an array of certificates owned by the dictionary.
    let certificates: &CFArray = unsafe { &*certificates.cast() };
    if CFArray::count(certificates) == 0 {
        return None;
    }
    // SAFETY: index zero exists by the count above, and the leaf is the first entry.
    let leaf = unsafe { CFArray::value_at_index(certificates, 0) };
    if leaf.is_null() {
        return None;
    }
    // SAFETY: the array holds certificates, borrowed for as long as the array is alive.
    let leaf: &SecCertificate = unsafe { &*leaf.cast() };
    // SAFETY: `leaf` is live for the call.
    unsafe { leaf.subject_summary() }.map(|name| name.to_string())
}

/// Without `kSecCSCheckAllArchitectures` (`1 << 0`, absent from these bindings) a byte flipped
/// in the other slice of the universal `/bin/zsh` still verified as signed by Apple.
fn flags() -> SecCSFlags {
    SecCSFlags::NoNetworkAccess | SecCSFlags(1)
}

#[cfg(test)]
mod tests {
    use std::fs::{copy, write};

    use super::*;

    const APPLE_SIGNED: &str = "/bin/zsh";

    #[test]
    fn a_shell_apple_ships_is_trusted_and_says_nothing_about_it() {
        let verdict = AppleTrust::new().verdict(Path::new(APPLE_SIGNED));

        assert_eq!(
            verdict,
            Verdict::Trusted {
                signer: Signer::Apple
            },
            "/bin/zsh is signed by Apple, which is what the Terminal row depends on"
        );
        assert!(
            verdict.settled(),
            "so the connection starts it without asking"
        );
        assert_eq!(
            verdict.note(),
            None,
            "and a listener hears nothing about it, which is the point"
        );
    }

    #[test]
    fn every_shell_this_mac_ships_is_trusted() {
        let trust = AppleTrust::new();

        for shell in [
            "/bin/bash",
            "/bin/csh",
            "/bin/dash",
            "/bin/ksh",
            "/bin/sh",
            "/bin/tcsh",
        ] {
            let program = Path::new(shell);
            if !program.is_file() {
                continue;
            }
            assert!(
                trust.verdict(program).settled(),
                "{shell} is signed by Apple and starts without a question"
            );
        }
    }

    #[test]
    fn a_file_nothing_signed_is_untrusted_and_says_so() {
        let file = std::env::temp_dir().join("acter-m2-unsigned");
        write(&file, b"#!/bin/sh\necho hello\n").expect("a temporary file");

        let verdict = AppleTrust::new().verdict(&file);

        assert!(
            !verdict.settled(),
            "so the connection asks before starting it"
        );
        assert!(
            matches!(
                verdict,
                Verdict::Untrusted {
                    fault: Fault::NotSigned | Fault::AdHoc
                }
            ),
            "nothing vouches for a file nobody signed: {verdict:?}"
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_shell_that_was_altered_after_signing_is_not_trusted() {
        let copied = std::env::temp_dir().join("acter-m2-altered-zsh");
        copy(APPLE_SIGNED, &copied).expect("a copy of a signed shell");
        let mut bytes = std::fs::read(&copied).expect("the copy reads back");
        let at = bytes.len() / 2;
        bytes[at] ^= 0xff;
        write(&copied, &bytes).expect("the copy is rewritten");

        let verdict = AppleTrust::new().verdict(&copied);

        assert!(
            !verdict.settled(),
            "a changed file is not the file Apple signed: {verdict:?}"
        );
        let _ = std::fs::remove_file(&copied);
    }

    #[test]
    fn a_file_replaced_under_the_same_name_is_checked_again() {
        let file = std::env::temp_dir().join("acter-m2-replaced");
        copy(APPLE_SIGNED, &file).expect("a copy of a signed shell");
        let trust = AppleTrust::new();
        assert!(trust.verdict(&file).settled(), "the copy is Apple's file");

        write(&file, b"not a shell at all").expect("the file is replaced");

        assert!(
            !trust.verdict(&file).settled(),
            "the replacement is a different file and gets a different answer"
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn nothing_is_known_about_a_file_nobody_has_checked() {
        let trust = AppleTrust::new();
        let program = Path::new(APPLE_SIGNED);

        assert_eq!(trust.known(program), None, "the list pays for nothing");

        let verdict = trust.verdict(program);

        assert_eq!(
            trust.known(program),
            Some(verdict),
            "and afterwards the same answer is free"
        );
    }

    #[test]
    fn a_file_that_is_not_there_is_unverifiable_rather_than_condemned() {
        let verdict = AppleTrust::new().verdict(Path::new("/bin/no-such-shell"));

        assert!(
            matches!(verdict, Verdict::Unverifiable { .. }),
            "{verdict:?}"
        );
        assert!(!verdict.settled());
    }
}
