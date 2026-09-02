//! Adapter: the conversation with Windows about one file — whether it trusts the signature,
//! and whose it is.
//!
//! **Both signing shapes, or it is worse than nothing** (spec B5.7, decision 5). Measured
//! 2026-08-27: `cmd.exe`, `powershell.exe` and `wsl.exe` are **catalog**-signed, not
//! embedded-signed, and re-measured across eight more System32 binaries on the same machine,
//! every one of which was catalog. A verification built the obvious way — `WinVerifyTrust`
//! over a `WINTRUST_FILE_INFO` — asks only whether the *file* carries a signature, so it
//! would report the unremovable Windows shell as unsigned. So the catalog is tried first, by
//! the file's hash, and an embedded signature is the fallback — which is where PowerShell 7
//! lives. Microsoft documents this exact fallback, and it is the one part of this design that
//! rests on supported guidance.
//!
//! **Who signed it is a second question and a different call.** `WinVerifyTrust` answers
//! whether this machine trusts the chain and says nothing about whose it is, so the subject
//! is read separately — from the catalog file for a catalog member, and from the file itself
//! for an embedded signature, because those are the two things that actually carry the
//! signature.
//!
//! **Revocation is bounded** (decision 8): `WTD_REVOKE_WHOLECHAIN` with
//! `WTD_CACHE_ONLY_URL_RETRIEVAL`, so a machine with no network answers from its cache
//! instead of hanging. A revocation answer that never comes is the unverifiable verdict. A
//! listener on a train is not under attack.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

use acter_core::{Fault, Signer, Verdict};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Cryptography::Catalog::{
    CATALOG_INFO, CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
};
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
    CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_INFO,
    CMSG_SIGNER_INFO_PARAM, CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext,
    CertGetNameStringW, CryptMsgClose, CryptMsgGetParam, CryptQueryObject, HCERTSTORE,
    PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_CATALOG, WTD_CHOICE_FILE,
    WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WinVerifyTrust,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, OPEN_EXISTING,
};

use super::wide;

/// `ERROR_CANT_ACCESS_FILE`, which is what opening an app execution alias answers.
///
/// **The wall the whole entry turns on** (spec B5.7, decision 4). Measured 2026-08-27:
/// `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe` is a zero-byte reparse point that
/// `Path::is_file()` reports as a file and that cannot be opened at all — so it can be
/// started and not read. Discovery resolves it to its package before it ever reaches here;
/// this constant is what says so when discovery could not.
const ERROR_CANT_ACCESS_FILE: u32 = 1920;

/// The statuses `WinVerifyTrust` answers with that this product has a sentence for.
///
/// **Written out rather than taken from a crate**, because `windows-sys` declares the
/// functions and not these `HRESULT`s, and because they are the fixtures the classification
/// is tested against: they are the only part of this file a test can reach without a signing
/// toolchain.
mod status {
    pub(super) const TRUST_E_NOSIGNATURE: i32 = 0x800B0100_u32 as i32;
    pub(super) const TRUST_E_BAD_DIGEST: i32 = 0x8009_6010_u32 as i32;
    pub(super) const TRUST_E_SUBJECT_NOT_TRUSTED: i32 = 0x800B_0004_u32 as i32;
    pub(super) const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B_0003_u32 as i32;
    pub(super) const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B_0001_u32 as i32;
    pub(super) const TRUST_E_EXPLICIT_DISTRUST: i32 = 0x800B_0111_u32 as i32;
    pub(super) const CERT_E_UNTRUSTEDROOT: i32 = 0x800B_0109_u32 as i32;
    pub(super) const CERT_E_UNTRUSTEDTESTROOT: i32 = 0x800B_010D_u32 as i32;
    pub(super) const CERT_E_CHAINING: i32 = 0x800B_010A_u32 as i32;
    pub(super) const CERT_E_REVOKED: i32 = 0x800B_010C_u32 as i32;
    pub(super) const CERT_E_EXPIRED: i32 = 0x800B_0101_u32 as i32;
    pub(super) const CRYPT_E_REVOCATION_OFFLINE: i32 = 0x8009_2013_u32 as i32;
    pub(super) const CRYPT_E_NO_REVOCATION_CHECK: i32 = 0x8009_2012_u32 as i32;
    pub(super) const CRYPT_E_SECURITY_SETTINGS: i32 = 0x8009_2026_u32 as i32;
}

/// The signer Microsoft's own certificates carry, as `CertGetNameStringW` renders it.
///
/// **A prefix rather than an exact name**, because the subject differs between the
/// certificates Microsoft signs with — `Microsoft Windows`, `Microsoft Corporation`,
/// `Microsoft Windows Publisher` — and all of them are Microsoft. Measured 2026-08-27:
/// the shells Windows ships report `CN=Microsoft Windows, O=Microsoft Corporation`, and the
/// Store PowerShell reports `CN=Microsoft Corporation`.
const MICROSOFT: &str = "Microsoft";

/// Verify this exact file, and say what Windows made of it.
pub(super) fn verify(program: &Path) -> Verdict {
    let path = wide(program.as_os_str());
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_READ,
            // Sharing delete as well as read, because this is a file somebody may be
            // replacing while we look at it — which is precisely the race this check
            // narrows and cannot close.
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Verdict::Unverifiable {
            why: unopenable(unsafe { GetLastError() }),
        };
    }

    let verdict = match catalog(file, &path) {
        Some(verdict) => verdict,
        None => embedded(file, &path, program),
    };
    unsafe { CloseHandle(file) };
    verdict
}

/// Why a file could not be opened, as a whole sentence — because it is read aloud after
/// "Acter could not check who signed this file."
fn unopenable(error: u32) -> String {
    match error {
        // The measured one, and the one with something to say: an app execution alias has no
        // readable signature and no readable anything.
        ERROR_CANT_ACCESS_FILE => "It is an app execution alias, which Windows does not let \
                                   anything read, and Acter could not work out which \
                                   package it points at."
            .to_owned(),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => "The file is not there any more.".to_owned(),
        other => format!("Windows would not open it, and gave error {other}."),
    }
}

/// The catalog path: hash the file, find a catalog claiming that hash, and verify through it.
///
/// `None` when no catalog on this machine claims the file — which is not a failure and is
/// where an embedded signature takes over.
fn catalog(file: HANDLE, path: &[u16]) -> Option<Verdict> {
    let algorithm = wide_str("SHA256");
    let mut admin: isize = 0;
    let acquired =
        unsafe { CryptCATAdminAcquireContext2(&mut admin, null(), algorithm.as_ptr(), null(), 0) };
    if acquired == 0 {
        return None;
    }
    let verdict = through_catalog(admin, file, path);
    unsafe { CryptCATAdminReleaseContext(admin, 0) };
    verdict
}

/// The same, with the catalog administrator context in hand so it is released exactly once.
fn through_catalog(admin: isize, file: HANDLE, path: &[u16]) -> Option<Verdict> {
    let mut length: u32 = 0;
    if unsafe { CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut length, null_mut(), 0) } == 0
        || length == 0
    {
        return None;
    }
    let mut hash = vec![0_u8; length as usize];
    if unsafe {
        CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut length, hash.as_mut_ptr(), 0)
    } == 0
    {
        return None;
    }

    let found =
        unsafe { CryptCATAdminEnumCatalogFromHash(admin, hash.as_ptr(), length, 0, null_mut()) };
    if found == 0 {
        return None;
    }
    let mut info = CATALOG_INFO {
        cbStruct: size_of::<CATALOG_INFO>() as u32,
        ..Default::default()
    };
    let described = unsafe { CryptCATCatalogInfoFromContext(found, &mut info, 0) } != 0;
    let verdict = described.then(|| {
        let catalogued = from_wide(&info.wszCatalogFile);
        // The member tag is the file's hash as hexadecimal, which is how a catalog names its
        // members — the same string `signtool` prints.
        let tag = wide_str(&hexadecimal(&hash));
        let mut member = WINTRUST_CATALOG_INFO {
            cbStruct: size_of::<WINTRUST_CATALOG_INFO>() as u32,
            dwCatalogVersion: 0,
            pcwszCatalogFilePath: info.wszCatalogFile.as_ptr(),
            pcwszMemberTag: tag.as_ptr(),
            pcwszMemberFilePath: path.as_ptr(),
            hMemberFile: file,
            pbCalculatedFileHash: hash.as_mut_ptr(),
            cbCalculatedFileHash: length,
            pcCatalogContext: null_mut(),
            hCatAdmin: admin,
        };
        let status = asked(
            WTD_CHOICE_CATALOG,
            WINTRUST_DATA_0 {
                pCatalog: &raw mut member,
            },
        );
        // **Whose signature it is lives on the catalog, not on the file.** A catalog member
        // carries no signature of its own — that is what makes it a catalog member — so
        // asking the file who signed it would answer "nobody" about something Microsoft
        // signed.
        verdict_for(status, || signer(Path::new(&catalogued)))
    });
    unsafe { CryptCATAdminReleaseCatalogContext(admin, found, 0) };
    verdict
}

/// The fallback: a signature carried by the file itself, which is where PowerShell 7 lives.
fn embedded(file: HANDLE, path: &[u16], program: &Path) -> Verdict {
    let mut about = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: file,
        pgKnownSubject: null_mut(),
    };
    let status = asked(
        WTD_CHOICE_FILE,
        WINTRUST_DATA_0 {
            pFile: &raw mut about,
        },
    );
    verdict_for(status, || signer(program))
}

/// One `WinVerifyTrust` call, opened and closed.
///
/// The state has to be closed as well as opened: `WTD_STATEACTION_VERIFY` allocates, and a
/// verification that never closes leaks for the life of the process — which for a check that
/// runs on every connection is a check that costs more the longer Acter is open.
fn asked(choice: u32, subject: WINTRUST_DATA_0) -> i32 {
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        // **Bounded, so a machine with no network answers from its cache rather than
        // hanging** (spec B5.7, decision 8). The whole chain is checked, and nothing is
        // fetched that is not already there.
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: choice,
        Anonymous: subject,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let status = unsafe { WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) };
    status
}

/// What a `WinVerifyTrust` status means, as the verdict a listener is told.
///
/// **Pure, and separated from every call above deliberately.** Producing a tampered or
/// untrusted-root file to assert against needs a signing certificate and a signing tool,
/// which no test in this repository has; the statuses themselves are the fixtures, and this
/// is the function they are asserted against (spec B5.7, definition of done).
///
/// `whose` is called only when it is needed, because reading a certificate subject is a
/// second pass over the file and most verdicts do not name anybody.
fn verdict_for(status: i32, whose: impl Fn() -> Option<String>) -> Verdict {
    match status {
        0 => Verdict::Trusted {
            signer: match whose() {
                Some(name) if name.contains(MICROSOFT) => Signer::Microsoft,
                Some(name) => Signer::Other { name },
                // Trusted with nobody readable behind it: Windows accepted the chain and the
                // subject could not be read, which is not the same as Microsoft having
                // signed it and must not be said as though it were.
                None => Signer::Other {
                    name: "somebody whose name could not be read".to_owned(),
                },
            },
        },
        status::TRUST_E_NOSIGNATURE => Verdict::Untrusted {
            fault: Fault::NotSigned,
        },
        status::TRUST_E_BAD_DIGEST => Verdict::Untrusted {
            fault: Fault::Tampered,
        },
        status::CERT_E_UNTRUSTEDROOT
        | status::CERT_E_UNTRUSTEDTESTROOT
        | status::CERT_E_CHAINING
        | status::TRUST_E_SUBJECT_NOT_TRUSTED
        | status::TRUST_E_EXPLICIT_DISTRUST => Verdict::Untrusted {
            fault: Fault::UntrustedRoot { signer: whose() },
        },
        status::CERT_E_REVOKED => Verdict::Untrusted {
            fault: Fault::Revoked { signer: whose() },
        },
        status::CERT_E_EXPIRED => Verdict::Untrusted {
            fault: Fault::Expired { signer: whose() },
        },
        // **A timeout is unverifiable rather than untrusted** (decision 8). A listener on a
        // train is not under attack, and saying they are would spend this product's one
        // alarming sentence on a train.
        status::CRYPT_E_REVOCATION_OFFLINE | status::CRYPT_E_NO_REVOCATION_CHECK => {
            Verdict::Unverifiable {
                why: "Windows could not check whether the signing certificate has been \
                      withdrawn, which usually means this computer is offline."
                    .to_owned(),
            }
        }
        status::TRUST_E_SUBJECT_FORM_UNKNOWN | status::TRUST_E_PROVIDER_UNKNOWN => {
            Verdict::Unverifiable {
                why: "Windows does not recognise this kind of file, so it has nowhere to look \
                      for a signature."
                    .to_owned(),
            }
        }
        status::CRYPT_E_SECURITY_SETTINGS => Verdict::Unverifiable {
            why: "This computer's policy stopped the check from finishing.".to_owned(),
        },
        other => Verdict::Unverifiable {
            why: format!("Windows answered with the code {:#010x}.", other as u32),
        },
    }
}

/// The certificate subject on a signed file, as `CertGetNameStringW` renders it.
///
/// **A second question and a different call** (decision 5): `WinVerifyTrust` says whether
/// this machine trusts the chain and never says whose it is. `None` for anything that cannot
/// be read, which the verdict then says plainly rather than filling in with a guess.
fn signer(program: &Path) -> Option<String> {
    let path = wide(program.as_os_str());
    let mut store: HCERTSTORE = null_mut();
    let mut message: *mut c_void = null_mut();
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.as_ptr().cast(),
            // **The two content types this product ever asks about, rather than "all"** —
            // and asking for "all" is what crashed here on 2026-08-27. A catalog is a
            // certificate trust list as well as a signed message, and `CryptQueryObject`
            // answered `CERT_QUERY_CONTENT_CTL` and set **no message handle**, so the next
            // call read through a null pointer. Naming the two shapes decision 5 is about —
            // a standalone signed message, which is what a catalog is, and an embedded one,
            // which is what a signed executable carries — makes it answer with the message.
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED | CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut store,
            &mut message,
            null_mut(),
        )
    };
    // **Both are checked, not just the return value.** A query can succeed and still hand
    // back nothing to read from — which is what happens for a file that is a certificate
    // rather than a signature over one.
    if queried == 0 || message.is_null() || store.is_null() {
        if !message.is_null() {
            unsafe { CryptMsgClose(message) };
        }
        if !store.is_null() {
            unsafe { CertCloseStore(store, 0) };
        }
        return None;
    }
    let name = subject(store, message);
    if !message.is_null() {
        unsafe { CryptMsgClose(message) };
    }
    if !store.is_null() {
        unsafe { CertCloseStore(store, 0) };
    }
    name
}

/// The signer's certificate, found in the store the message came with, and its display name.
fn subject(store: HCERTSTORE, message: *mut c_void) -> Option<String> {
    let mut length: u32 = 0;
    if unsafe { CryptMsgGetParam(message, CMSG_SIGNER_INFO_PARAM, 0, null_mut(), &mut length) } == 0
        || length == 0
    {
        return None;
    }
    // Held as `u64`s rather than bytes because what goes in it is a C structure full of
    // pointers, and a `Vec<u8>` is only guaranteed to be aligned for a byte.
    let mut held = vec![0_u64; (length as usize).div_ceil(size_of::<u64>())];
    if unsafe {
        CryptMsgGetParam(
            message,
            CMSG_SIGNER_INFO_PARAM,
            0,
            held.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return None;
    }

    // Safety: `CryptMsgGetParam` filled this buffer with exactly this structure, at the size
    // it asked for a moment ago.
    let signed = unsafe { &*held.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    // Kept in a local rather than built inline: `CertFindCertificateInStore` is handed a
    // pointer into it, and a temporary would be gone before the call returned.
    let looking = CERT_INFO {
        Issuer: signed.Issuer,
        SerialNumber: signed.SerialNumber,
        ..Default::default()
    };
    let certificate = unsafe {
        CertFindCertificateInStore(
            store,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_SUBJECT_CERT,
            (&raw const looking).cast(),
            null(),
        )
    };
    if certificate.is_null() {
        return None;
    }
    let name = display_name(certificate);
    unsafe { CertFreeCertificateContext(certificate) };
    name
}

/// The certificate's simple display name, asked for its length first.
fn display_name(certificate: *const CERT_CONTEXT) -> Option<String> {
    let length = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            null_mut(),
            0,
        )
    };
    if length <= 1 {
        return None;
    }
    let mut name = vec![0_u16; length as usize];
    let written = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            name.as_mut_ptr(),
            length,
        )
    };
    if written <= 1 {
        return None;
    }
    Some(from_wide(&name))
}

/// A Rust string as the null-terminated wide string every call here takes.
fn wide_str(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The other direction: a fixed-size wide buffer as a string, stopping at the terminator.
fn from_wide(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

/// A hash as the upper-case hexadecimal a catalog names its members by.
fn hexadecimal(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{byte:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The classification, asserted against the statuses themselves** — which are the only
    /// fixtures available: producing a tampered file or one signed by an untrusted root needs
    /// a signing certificate and a signing tool, and nothing in this repository has either.
    /// Every status this product claims to understand is here, so a mapping cannot be changed
    /// without changing what a listener is told.
    #[test]
    fn every_status_windows_answers_with_is_told_apart() {
        let nobody = || None;

        assert!(matches!(
            verdict_for(status::TRUST_E_NOSIGNATURE, nobody),
            Verdict::Untrusted {
                fault: Fault::NotSigned
            }
        ));
        assert!(matches!(
            verdict_for(status::TRUST_E_BAD_DIGEST, nobody),
            Verdict::Untrusted {
                fault: Fault::Tampered
            }
        ));
        for untrusted in [
            status::CERT_E_UNTRUSTEDROOT,
            status::CERT_E_UNTRUSTEDTESTROOT,
            status::CERT_E_CHAINING,
            status::TRUST_E_SUBJECT_NOT_TRUSTED,
            status::TRUST_E_EXPLICIT_DISTRUST,
        ] {
            assert!(
                matches!(
                    verdict_for(untrusted, nobody),
                    Verdict::Untrusted {
                        fault: Fault::UntrustedRoot { .. }
                    }
                ),
                "{untrusted:#010x} is a chain this machine will not follow"
            );
        }
        assert!(matches!(
            verdict_for(status::CERT_E_REVOKED, nobody),
            Verdict::Untrusted {
                fault: Fault::Revoked { .. }
            }
        ));
        assert!(matches!(
            verdict_for(status::CERT_E_EXPIRED, nobody),
            Verdict::Untrusted {
                fault: Fault::Expired { .. }
            }
        ));
    }

    /// **Decision 8, and the one that keeps a listener on a train out of an alarming
    /// dialog.** A revocation check that could not run is unverifiable, never untrusted.
    #[test]
    fn a_revocation_check_that_could_not_run_is_unverifiable_rather_than_untrusted() {
        for offline in [
            status::CRYPT_E_REVOCATION_OFFLINE,
            status::CRYPT_E_NO_REVOCATION_CHECK,
        ] {
            let verdict = verdict_for(offline, || None);

            assert!(
                matches!(verdict, Verdict::Unverifiable { .. }),
                "{offline:#010x} is not an accusation"
            );
            assert!(verdict.said().contains("offline"), "and it says why");
        }
    }

    /// A status nobody wrote a sentence for is still unverifiable with a reason, rather than
    /// being read as either of the other two — which is decision 4's rule applied to the case
    /// nobody anticipated.
    #[test]
    fn a_status_nobody_anticipated_is_unverifiable_and_says_what_windows_answered() {
        let verdict = verdict_for(0x8007_0005_u32 as i32, || None);

        let Verdict::Unverifiable { why } = verdict else {
            panic!("an unknown status is never trusted and never condemned");
        };
        assert!(
            why.contains("0x80070005"),
            "and carries what was said: {why}"
        );
    }

    /// **Decision 5's two sentences.** Microsoft's certificates do not all carry the same
    /// subject, so the one thing they share is what is matched — and anybody else is named.
    #[test]
    fn microsoft_and_somebody_else_are_two_different_verdicts() {
        assert_eq!(
            verdict_for(0, || Some("Microsoft Windows".to_owned())),
            Verdict::Trusted {
                signer: Signer::Microsoft
            }
        );
        assert_eq!(
            verdict_for(0, || Some("Microsoft Corporation".to_owned())),
            Verdict::Trusted {
                signer: Signer::Microsoft
            }
        );
        assert_eq!(
            verdict_for(0, || Some("Contoso Corporation".to_owned())),
            Verdict::Trusted {
                signer: Signer::Other {
                    name: "Contoso Corporation".to_owned()
                }
            }
        );
    }

    /// Trusted with nobody readable behind it is **not** Microsoft, and saying so is the
    /// point: the whole value of the check is that "signed by Microsoft" means it.
    #[test]
    fn a_trusted_file_whose_signer_cannot_be_read_is_not_reported_as_microsoft() {
        let verdict = verdict_for(0, || None);

        assert_ne!(
            verdict,
            Verdict::Trusted {
                signer: Signer::Microsoft
            }
        );
        assert!(verdict.settled(), "this machine does trust it");
    }

    /// The member tag a catalog names a file by, which is the hash in the spelling
    /// `signtool` prints.
    #[test]
    fn a_hash_is_named_the_way_a_catalog_names_it() {
        assert_eq!(hexadecimal(&[0x0a, 0xff, 0x10]), "0AFF10");
    }

    /// A fixed-size buffer from Windows is a string up to its terminator and not one byte
    /// further — the 260-character catalog path is nearly all padding.
    #[test]
    fn a_windows_buffer_ends_where_its_terminator_is() {
        let mut buffer = [0_u16; 8];
        for (at, unit) in "cat".encode_utf16().enumerate() {
            buffer[at] = unit;
        }

        assert_eq!(from_wide(&buffer), "cat");
    }

    /// **The measured one** (decision 4): an app execution alias cannot be opened, and what
    /// a listener hears about it names what it is rather than a Windows error number.
    #[test]
    fn a_file_that_cannot_be_opened_says_which_kind_of_cannot() {
        assert!(unopenable(ERROR_CANT_ACCESS_FILE).contains("app execution alias"));
        assert!(unopenable(ERROR_FILE_NOT_FOUND).contains("not there"));
        assert!(unopenable(5).contains("error 5"));
        for said in [
            unopenable(ERROR_CANT_ACCESS_FILE),
            unopenable(ERROR_FILE_NOT_FOUND),
            unopenable(5),
        ] {
            assert!(said.ends_with('.'), "it is read aloud, so it ends: {said}");
        }
    }
}
