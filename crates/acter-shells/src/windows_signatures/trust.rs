//! Adapter: the conversation with Windows about one file — whether it trusts the signature,
//! and whose it is.
//!
//! On Windows 11 Pro 26200 `cmd.exe`, `powershell.exe` and `wsl.exe` are catalog-signed, not
//! embedded-signed, so the catalog is tried before the file's own signature.

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

/// What opening an app execution alias fails with; see windows_signatures/alias.rs.
const ERROR_CANT_ACCESS_FILE: u32 = 1920;

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

/// Matched anywhere in the subject, because Microsoft signs as both `Microsoft Windows` and
/// `Microsoft Corporation`.
const MICROSOFT: &str = "Microsoft";

pub(super) fn verify(program: &Path) -> Verdict {
    let path = wide(program.as_os_str());
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_READ,
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

/// A whole sentence, read aloud after "Acter could not check who signed this file."
fn unopenable(error: u32) -> String {
    match error {
        ERROR_CANT_ACCESS_FILE => "It is an app execution alias, which Windows does not let \
                                   anything read, and Acter could not work out which \
                                   package it points at."
            .to_owned(),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => "The file is not there any more.".to_owned(),
        other => format!("Windows would not open it, and gave error {other}."),
    }
}

/// `None` when no catalog on this machine claims the file, and the embedded signature is tried.
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
        // A catalog names a member by its hash in upper-case hexadecimal.
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
        // A catalog member carries no signature of its own, so the signer is read from the catalog.
        verdict_for(status, || signer(Path::new(&catalogued)))
    });
    unsafe { CryptCATAdminReleaseCatalogContext(admin, found, 0) };
    verdict
}

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

/// The state must be closed as well as opened, or `WTD_STATEACTION_VERIFY` leaks for the life
/// of the process.
fn asked(choice: u32, subject: WINTRUST_DATA_0) -> i32 {
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
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

fn verdict_for(status: i32, whose: impl Fn() -> Option<String>) -> Verdict {
    match status {
        0 => Verdict::Trusted {
            signer: match whose() {
                Some(name) if name.contains(MICROSOFT) => Signer::Microsoft,
                Some(name) => Signer::Other { name },
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

/// `None` for a file whose certificate subject cannot be read.
fn signer(program: &Path) -> Option<String> {
    let path = wide(program.as_os_str());
    let mut store: HCERTSTORE = null_mut();
    let mut message: *mut c_void = null_mut();
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.as_ptr().cast(),
            // Asking for every content type crashed: a catalog then answers
            // `CERT_QUERY_CONTENT_CTL` with no message handle.
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
    // A query can succeed and still hand back no message or no store.
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

fn subject(store: HCERTSTORE, message: *mut c_void) -> Option<String> {
    let mut length: u32 = 0;
    if unsafe { CryptMsgGetParam(message, CMSG_SIGNER_INFO_PARAM, 0, null_mut(), &mut length) } == 0
        || length == 0
    {
        return None;
    }
    // `u64`s so the buffer is aligned for a structure full of pointers.
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

    // Safety: `CryptMsgGetParam` filled this buffer with exactly this structure.
    let signed = unsafe { &*held.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    // A local, because `CertFindCertificateInStore` is handed a pointer into it.
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

fn wide_str(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn hexadecimal(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{byte:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_hash_is_named_the_way_a_catalog_names_it() {
        assert_eq!(hexadecimal(&[0x0a, 0xff, 0x10]), "0AFF10");
    }

    #[test]
    fn a_windows_buffer_ends_where_its_terminator_is() {
        let mut buffer = [0_u16; 8];
        for (at, unit) in "cat".encode_utf16().enumerate() {
            buffer[at] = unit;
        }

        assert_eq!(from_wide(&buffer), "cat");
    }

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
