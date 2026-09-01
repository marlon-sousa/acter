//! Adapter: who signed the files a Mac would start, behind acter-core's `Signatures` port.
//!
//! **The one place in this product where being wrong is a security answer**, which is why
//! this asks the Security framework rather than a wrapper: `objc2-security` is Apple's own
//! declarations, the same choice `windows-sys` is on the other side of this crate, and a
//! small low-traffic crate is a poor thing to put here (spec B5.7, decision 5, applied to a
//! second platform).
//!
//! **It ships with the Terminal row rather than after it, and that is a product decision**
//! (DESIGN, decided 2026-08-31). Off Windows the port falls back to acter-core's `Unchecked`,
//! which vouches for nothing — the right refusal, and the wrong thing to ship on its own the
//! moment a local row exists, because then every connection to a `/bin/zsh` that Apple signed
//! raises a security dialog. A dialog that always fires is a dialog a listener learns to
//! dismiss, and it is the one dialog in this product that is about security.
//!
//! # The ladder, and why it is a ladder
//!
//! A valid signature is not a trusted one, so validity is asked first and identity second:
//!
//! 1. **Is the signature intact?** `SecStaticCodeCheckValidity` with no requirement. Its
//!    refusals are the faults — unsigned, or hashes that no longer match the file.
//! 2. **Is Apple the anchor?** The requirement `anchor apple`, which is satisfied only by the
//!    binaries Apple itself ships. Measured 2026-09-01: all seven shells in this Mac's
//!    `/etc/shells` satisfy it, with the leaf certificate "Software Signing".
//! 3. **Did Apple at least issue the certificate?** `anchor apple generic`, which is what a
//!    Developer ID or Mac App Store signature satisfies. That is a real third party this
//!    machine has a reason to trust, and it is named rather than merely accepted.
//! 4. **Otherwise there is a certificate this machine has no reason to trust, or there is no
//!    certificate at all** — and those are two different sentences: somebody signed it with
//!    something unknown, or the file signed itself.
//!
//! **Step 4's second half is `Fault::AdHoc`, and it is why that variant exists.** Apple
//! silicon requires every executable to carry at least an ad-hoc signature, so a locally
//! built or Homebrew-installed shell has one: unaltered since it was built, with nothing
//! saying who built it. Calling that "nothing has signed this file" would be false about the
//! half that is true.
//!
//! **Nothing here reaches the network**, which is B5.7 decision 7 on this platform:
//! `kSecCSNoNetworkAccess` is passed with every check, so a connection cannot stall on
//! somebody else's revocation server in front of a listener who is waiting.
//!
//! **What is remembered, and why by more than the path.** As on Windows: the verdict is
//! cached by resolved path, size and last-write time for the life of the process, so a file
//! *replaced* between two connections is checked again rather than vouched for by what its
//! predecessor was.

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

/// The requirement every binary Apple ships satisfies, and nothing else does.
const ANCHOR_APPLE: &str = "anchor apple";

/// The requirement a Developer ID or Mac App Store signature satisfies: Apple issued the
/// certificate, somebody else holds it.
const ANCHOR_APPLE_GENERIC: &str = "anchor apple generic";

/// This machine's own answer about a file, remembered for the life of the process.
#[derive(Debug, Default)]
pub struct AppleTrust {
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

impl AppleTrust {
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

    /// **Nothing is verified here** (spec B5.7, decision 7), which is what lets the connect
    /// list name a verdict without paying for one. Reading the file's size and time is a
    /// `stat` and reaches no certificate store and no network.
    fn known(&self, program: &Path) -> Option<Verdict> {
        self.recall(program, stamp(program))
    }
}

/// The file as it stands right now, or `None` for one that cannot be looked at at all.
fn stamp(program: &Path) -> Option<Stamp> {
    let about = std::fs::metadata(program).ok()?;
    Some(Stamp {
        size: about.len(),
        modified: about.modified().ok(),
    })
}

/// The whole ladder, for one file.
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
        // Apple issued the certificate and its subject could not be read, which is not a
        // sentence anybody can act on: this machine trusts the chain and cannot say whose it
        // is, so it says that rather than vouching for a name it does not have.
        (true, None) => Verdict::Unverifiable {
            why: "macOS trusts this file's signature but could not say who holds it.".to_owned(),
        },
        (false, signer @ Some(_)) => Verdict::Untrusted {
            fault: Fault::UntrustedRoot { signer },
        },
        // A signature with no certificate behind it at all.
        (false, None) => Verdict::Untrusted {
            fault: Fault::AdHoc,
        },
    }
}

/// The file, as something the Security framework will answer questions about.
fn static_code(program: &Path) -> Option<CFRetained<SecStaticCode>> {
    let path = CFString::from_str(&program.to_string_lossy());
    let url = CFURL::with_file_system_path(
        None,
        Some(&path),
        CFURLPathStyle::CFURLPOSIXPathStyle,
        false,
    )?;
    let mut code: *const SecStaticCode = std::ptr::null();
    // SAFETY: `url` outlives the call, and `code` is a pointer this stack frame owns. A
    // non-zero status or a null pointer is the file having no code to talk about, which is
    // the `None` below rather than a verdict.
    // **Default flags, and every `Create` call in this file is the same**: the framework
    // refuses a flag it does not expect here, which is what made this return nothing at all
    // the first time it was written with the checking flags passed through (measured
    // 2026-09-01). Only the checks below take [`flags`].
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

/// Whether the signature is intact, and what to say when it is not.
///
/// **Only the two refusals that name something a listener can act on become a fault**;
/// anything else is Acter not being able to tell, which is the third outcome this product has
/// precisely so that it never has to choose between "trusted" and "condemned" when it does
/// not know (spec B5.7, decision 4).
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

/// Whether this file satisfies one code-signing requirement.
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

/// The subject of the certificate this file was signed with, when there is one.
///
/// `None` is what an ad-hoc signature answers, because an ad-hoc signature has no
/// certificates at all — which is what makes it tellable from every other outcome.
fn leaf_name(code: &SecStaticCode) -> Option<String> {
    let mut information: *const CFDictionary = std::ptr::null();
    // SAFETY: `code` is live and `information` is owned by this frame. The flag asks for the
    // signing information, which is the documented way to reach the certificate chain.
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
    // SAFETY: the framework documents this key's value as an array of certificates, and it
    // lives as long as the dictionary it came out of.
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

/// The flags the two checks are made with.
///
/// **`kSecCSNoNetworkAccess` is B5.7 decision 7 on this platform**: this runs on the
/// connection, in front of a listener who is waiting, and a check that can reach somebody
/// else's server can take as long as that server likes.
///
/// **`kSecCSCheckAllArchitectures` is a measurement.** Every shell macOS ships is a universal
/// binary — `/bin/zsh` holds an x86_64 slice and an arm64e one — and the default check reads
/// only the slice this machine would run. Measured 2026-09-01 by flipping one byte in the
/// middle of a copy of `/bin/zsh`: without this flag the altered file still verified as
/// signed by Apple, because the byte was in the other architecture's slice. `codesign -v`
/// checks every slice, so a user checking by hand would have been told what Acter was not.
///
/// It is spelled as a bit because these bindings do not carry it: it is
/// `kSecCSCheckAllArchitectures`, `1 << 0`, declared for `SecStaticCodeCheckValidity` in
/// `CSCommon.h`.
fn flags() -> SecCSFlags {
    SecCSFlags::NoNetworkAccess | SecCSFlags(1)
}

#[cfg(test)]
mod tests {
    use std::fs::{copy, write};

    use super::*;

    /// A shell every Mac has, which is also one of the shells the Terminal row offers.
    const APPLE_SIGNED: &str = "/bin/zsh";

    /// **The measurement the whole entry turns on** (spec M2, decision 5, measured
    /// 2026-09-01): the ordinary case is trusted, signed by Apple, and therefore says
    /// nothing at connection time.
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

    /// Every shell this Mac offers, not just the one: a single trusted entry beside six
    /// dialogs would be the experience DESIGN says this entry exists to prevent.
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

    /// A file with no signature at all is untrusted and says which way: this is what a
    /// script or an ordinary data file put where a shell should be looks like.
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

    /// **A signed file that is then changed stops being trusted**, which is the check
    /// earning its place: the whole point is to notice a file that is not the one whoever
    /// signed it produced.
    #[test]
    fn a_shell_that_was_altered_after_signing_is_not_trusted() {
        let copied = std::env::temp_dir().join("acter-m2-altered-zsh");
        copy(APPLE_SIGNED, &copied).expect("a copy of a signed shell");
        let mut bytes = std::fs::read(&copied).expect("the copy reads back");
        // One byte, well inside the text the signature covers.
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

    /// **The cache is about the file, not about its name** — so a path whose contents changed
    /// is asked again rather than answered from what used to be there.
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

    /// **Nothing is known until something has been verified** (spec B5.7, decisions 6 and 7),
    /// which is what lets the connect list name a verdict it did not pay for without ever
    /// stalling to produce one.
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

    /// A path with nothing at it is not a verdict about a file: it is Acter being unable to
    /// tell, which is the third outcome and never one of the other two.
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
