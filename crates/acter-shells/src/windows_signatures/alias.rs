//! Adapter: what an app execution alias actually points at.
//!
//! On Windows 11 Pro 26200 `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe` is a zero-byte reparse
//! point that `Path::is_file()` accepts and that fails to open with `ERROR_CANT_ACCESS_FILE`.
//! The `IO_REPARSE_TAG_APPEXECLINK` buffer layout is undocumented, so resolving can fail.

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

const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;

/// The fixed part of `REPARSE_DATA_BUFFER`: a tag, a length and a reserved word.
const HEADER: usize = 8;

/// The `ULONG Version` the app-exec-link payload starts with, before its strings.
const VERSION: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppExecLink {
    pub(crate) family: String,
    pub(crate) program: PathBuf,
}

/// `None` for a file that is not an alias, and for an alias that will not resolve.
pub(crate) fn target(program: &Path) -> Option<AppExecLink> {
    let path = wide(program.as_os_str());
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            // Read access is what fails on an alias.
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
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

fn parse(buffer: &[u8]) -> Option<AppExecLink> {
    let tag = u32::from_le_bytes(buffer.get(..4)?.try_into().ok()?);
    if tag != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let payload = buffer.get(HEADER + VERSION..)?;
    let mut strings = utf16_strings(payload);
    // The four strings are the package family, the application id, the executable and a type.
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

fn utf16_strings(payload: &[u8]) -> impl Iterator<Item = String> + '_ {
    let units: Vec<u16> = payload
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
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

    fn payload(strings: &[&str]) -> Vec<u8> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IO_REPARSE_TAG_APPEXECLINK.to_le_bytes());
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

    #[test]
    fn a_reparse_point_of_another_kind_is_not_an_execution_alias() {
        let mut buffer = payload(&["something", "else", "entirely", "0"]);
        buffer[..4].copy_from_slice(&0xA000_000C_u32.to_le_bytes());

        assert_eq!(parse(&buffer), None, "that tag is a symbolic link");
    }

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
