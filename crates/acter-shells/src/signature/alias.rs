//! Adapter: what an app execution alias actually points at.
//!
//! **This is the wall the whole entry turns on** (spec B5.7, decision 4). Measured
//! 2026-08-27: `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe` is a zero-byte reparse point.
//! Rust's `Path::is_file()` returns `true` for it and `fs::metadata` returns `Ok(0)`, so the
//! old lookup found it and would have offered it — and opening it fails with
//! `ERROR_CANT_ACCESS_FILE`, so it has no readable signature and no readable anything.
//! Resolve-then-verify does not compose with a file that cannot be read, so the alias is
//! resolved *first*, to the package file it stands for, and that file is what is checked and
//! started.
//!
//! **A measured dependency and not a documented contract.** `IO_REPARSE_TAG_APPEXECLINK`'s
//! buffer layout is not in the official documentation; it is described by
//! reverse-engineering write-ups, it is what `CreateProcess` itself resolves through
//! `LoadAppExecutionAliasInfoEx`, and PowerShell added support for the same tag in its own
//! filesystem provider. So the resolution is *attempted*, its failure is an ordinary outcome,
//! and a candidate that cannot be resolved is reported unverifiable with its reason — never
//! quietly trusted and never quietly condemned.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_APPEXECLINK;

use super::wide;

/// What Windows will put in a reparse buffer, which is the size every example uses.
const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;

/// The fixed part of `REPARSE_DATA_BUFFER`: a tag, a length and a reserved word.
const HEADER: usize = 8;

/// The `ULONG Version` the app-exec-link payload starts with, before its strings.
const VERSION: usize = 4;

/// Where an execution alias points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppExecLink {
    /// The package family, `Microsoft.PowerShell_8wekyb3d8bbwe` — the identity
    /// `CreateProcess` resolves the alias through, and what tells two aliases of the same
    /// program apart.
    pub(crate) family: String,
    /// The executable the alias stands for, which is a real file with a real signature.
    pub(crate) program: PathBuf,
}

/// Where this alias points, or `None` for anything that is not one.
///
/// **Not an error type**, because "this is an ordinary file" is by far the commonest answer
/// and is not a failure. A file that *is* an alias and will not resolve is also `None`; what
/// happens then is that verification opens it, fails, and says so (decision 4's third
/// preference, unverifiable with its reason).
pub(crate) fn target(program: &Path) -> Option<AppExecLink> {
    let path = wide(program.as_os_str());
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            // Attributes only: the point is that the *contents* cannot be read, and asking
            // for read access is what fails on an alias.
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            // The first flag is what stops Windows following the link for us — following it
            // is exactly what we are trying to do by hand. The second is needed because a
            // reparse point may be a directory.
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return None;
    }

    let mut buffer = vec![0_u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    let mut returned: u32 = 0;
    let read = unsafe {
        DeviceIoControl(
            file,
            FSCTL_GET_REPARSE_POINT,
            null(),
            0,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
            &mut returned,
            null_mut(),
        )
    };
    unsafe { CloseHandle(file) };
    if read == 0 {
        return None;
    }
    buffer.truncate(returned as usize);
    parse(&buffer)
}

/// The payload, as the two strings this product needs from it.
///
/// Separated from the call so the layout — which is the measured, undocumented part — can be
/// asserted against bytes a test writes, rather than against whatever this machine has
/// installed.
fn parse(buffer: &[u8]) -> Option<AppExecLink> {
    let tag = u32::from_le_bytes(buffer.get(..4)?.try_into().ok()?);
    if tag != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let payload = buffer.get(HEADER + VERSION..)?;
    let mut strings = utf16_strings(payload);
    // The four strings, in the order the tag lays them out: the package family, the
    // application id, the executable, and a type. Only the first and the third are facts
    // about *which file this is*, which is all this product is asking.
    let family = strings.next()?;
    let _application = strings.next()?;
    let program = strings.next()?;
    if family.is_empty() || program.is_empty() {
        return None;
    }
    Some(AppExecLink {
        family,
        program: PathBuf::from(program),
    })
}

/// The null-terminated wide strings packed one after another in the payload.
fn utf16_strings(payload: &[u8]) -> impl Iterator<Item = String> + '_ {
    let units: Vec<u16> = payload
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    units
        .split(|unit| *unit == 0)
        .map(String::from_utf16_lossy)
        .collect::<Vec<String>>()
        .into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload in the shape the tag lays one out, written by the test rather than read off
    /// a machine — which is the only way to assert a layout nobody documented.
    fn payload(strings: &[&str]) -> Vec<u8> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IO_REPARSE_TAG_APPEXECLINK.to_le_bytes());
        // The length and the reserved word, which this parser does not read but which are
        // where the payload starts from.
        buffer.extend_from_slice(&0_u16.to_le_bytes());
        buffer.extend_from_slice(&0_u16.to_le_bytes());
        buffer.extend_from_slice(&3_u32.to_le_bytes());
        for string in strings {
            for unit in string.encode_utf16() {
                buffer.extend_from_slice(&unit.to_le_bytes());
            }
            buffer.extend_from_slice(&0_u16.to_le_bytes());
        }
        buffer
    }

    /// **The measured layout** (decision 4): a package family, an application id, the
    /// executable, and a type — of which the first and the third are what say which file
    /// this alias really is.
    #[test]
    fn an_execution_alias_names_its_package_and_the_file_it_stands_for() {
        let buffer = payload(&[
            "Microsoft.PowerShell_8wekyb3d8bbwe",
            "Microsoft.PowerShell_8wekyb3d8bbwe!PowerShell",
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe",
            "0",
        ]);

        assert_eq!(
            parse(&buffer),
            Some(AppExecLink {
                family: "Microsoft.PowerShell_8wekyb3d8bbwe".to_owned(),
                program: PathBuf::from(
                    r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe"
                ),
            })
        );
    }

    /// **An ordinary reparse point is not one of these**, and reading one as though it were
    /// would produce a path out of a symlink's substitute name and start something nobody
    /// chose. The tag is the only thing that says which kind this is.
    #[test]
    fn a_reparse_point_of_another_kind_is_not_an_execution_alias() {
        let mut buffer = payload(&["something", "else", "entirely", "0"]);
        buffer[..4].copy_from_slice(&0xA000_000C_u32.to_le_bytes());

        assert_eq!(parse(&buffer), None, "that tag is a symbolic link");
    }

    /// **Failing to resolve is an ordinary outcome** (decision 4), so a payload that stops
    /// early answers nothing rather than half a path.
    #[test]
    fn a_payload_that_stops_early_resolves_to_nothing() {
        assert_eq!(parse(&[]), None);
        assert_eq!(
            parse(&payload(&["Microsoft.PowerShell_8wekyb3d8bbwe"])),
            None
        );
        assert_eq!(parse(&payload(&["", "", ""])), None);
    }
}
