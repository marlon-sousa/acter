//! Adapter: the places a shell is installed that `PATH` does not necessarily name.
//!
//! On Windows 11 Pro 26200 `HKLM\SOFTWARE\Microsoft\PowerShell\3\PowerShellEngine` reports
//! 5.1.26100.8875, and `HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions` is absent
//! when PowerShell 7.6.5 came from the Store.

use std::path::{Path, PathBuf};

use super::{POWERSHELL, environment};

const PWSH: &str = "pwsh";
const WINDOWS_POWERSHELL: &str = "powershell";

const PROGRAM_FILES: [&str; 3] = ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432"];

const DOTNET_TOOLS: [&str; 2] = [".dotnet", "tools"];

const WINDOWS_POWERSHELL_DIRECTORY: [&str; 2] = ["WindowsPowerShell", "v1.0"];

pub(super) fn known(program: &str) -> Vec<PathBuf> {
    let file = file_name(program);
    match stem(program).as_str() {
        PWSH => powershell_seven(&file),
        WINDOWS_POWERSHELL => windows_powershell(&file),
        _ => Vec::new(),
    }
}

fn powershell_seven(file: &str) -> Vec<PathBuf> {
    let environment = environment();
    let mut candidates = Vec::new();
    for name in PROGRAM_FILES {
        let Some(root) = at(&environment, name) else {
            continue;
        };
        let Ok(versions) = std::fs::read_dir(root.join(POWERSHELL)) else {
            continue;
        };
        for version in versions.flatten() {
            candidates.push(version.path().join(file));
        }
    }
    if let Some(home) = at(&environment, "USERPROFILE") {
        let tools = DOTNET_TOOLS
            .iter()
            .fold(home, |directory, part| directory.join(part));
        candidates.push(tools.join(file));
    }
    candidates
}

fn windows_powershell(file: &str) -> Vec<PathBuf> {
    let environment = environment();
    at(&environment, "SYSTEMROOT")
        .map(|system| {
            let mut directory = system.join("System32");
            for part in WINDOWS_POWERSHELL_DIRECTORY {
                directory = directory.join(part);
            }
            vec![directory.join(file)]
        })
        .unwrap_or_default()
}

pub(super) fn registered(program: &str) -> Vec<(PathBuf, Option<String>)> {
    let file = file_name(program);
    match stem(program).as_str() {
        WINDOWS_POWERSHELL => windows_powershell_engine(&file),
        PWSH => powershell_core(&file),
        _ => Vec::new(),
    }
}

#[cfg(windows)]
fn windows_powershell_engine(file: &str) -> Vec<(PathBuf, Option<String>)> {
    let key = r"SOFTWARE\Microsoft\PowerShell\3\PowerShellEngine";
    registry::value(key, "ApplicationBase")
        .map(|base| {
            vec![(
                PathBuf::from(base).join(file),
                registry::value(key, "PowerShellVersion"),
            )]
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn powershell_core(file: &str) -> Vec<(PathBuf, Option<String>)> {
    let key = r"SOFTWARE\Microsoft\PowerShellCore\InstalledVersions";
    registry::children(key)
        .into_iter()
        .filter_map(|installed| {
            let under = format!(r"{key}\{installed}");
            let location = registry::value(&under, "InstallLocation")?;
            Some((
                PathBuf::from(location).join(file),
                registry::value(&under, "SemanticVersion"),
            ))
        })
        .collect()
}

#[cfg(not(windows))]
fn windows_powershell_engine(_file: &str) -> Vec<(PathBuf, Option<String>)> {
    Vec::new()
}

#[cfg(not(windows))]
fn powershell_core(_file: &str) -> Vec<(PathBuf, Option<String>)> {
    Vec::new()
}

fn at(environment: &[(String, PathBuf)], name: &str) -> Option<PathBuf> {
    environment
        .iter()
        .find(|(named, _)| named == name)
        .map(|(_, value)| value.clone())
}

fn file_name(program: &str) -> String {
    let named = Path::new(program);
    match named.extension() {
        Some(_) => named.file_name().map_or_else(
            || program.to_owned(),
            |file| file.to_string_lossy().into_owned(),
        ),
        None => format!("{}.exe", stem(program)),
    }
}

fn stem(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(windows)]
mod registry {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegGetValueW, RegOpenKeyExW,
    };

    /// Windows limits a key name to 255 characters.
    const NAME: usize = 256;

    /// `None` for a key or value that is not there.
    pub(super) fn value(key: &str, name: &str) -> Option<String> {
        let key = wide(key);
        let name = wide(name);
        let mut length: u32 = 0;
        let sized = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                &mut length,
            )
        };
        if sized != ERROR_SUCCESS || length == 0 {
            return None;
        }
        let mut held = vec![0_u16; (length as usize).div_ceil(2)];
        let read = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                held.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if read != ERROR_SUCCESS {
            return None;
        }
        Some(string(&held))
    }

    /// Empty for a key that is not there.
    pub(super) fn children(key: &str) -> Vec<String> {
        let mut opened: HKEY = null_mut();
        let path = wide(key);
        if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut opened) }
            != ERROR_SUCCESS
        {
            return Vec::new();
        }
        let mut names = Vec::new();
        let mut index = 0;
        loop {
            let mut name = [0_u16; NAME];
            let mut length = name.len() as u32;
            let read = unsafe {
                RegEnumKeyExW(
                    opened,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if read != ERROR_SUCCESS {
                break;
            }
            names.push(string(&name[..length as usize]));
            index += 1;
        }
        unsafe { RegCloseKey(opened) };
        names
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn string(value: &[u16]) -> String {
        let end = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_with_no_extension_still_names_a_file() {
        assert_eq!(file_name("pwsh"), "pwsh.exe");
        assert_eq!(file_name("pwsh.exe"), "pwsh.exe");
        // Joined because `Path` splits only on the host's separator.
        let full: PathBuf = ["tools", "pwsh", "pwsh.exe"].iter().collect();
        assert_eq!(file_name(&full.display().to_string()), "pwsh.exe");
    }

    #[test]
    fn a_program_with_nowhere_else_to_be_has_no_known_roots() {
        assert!(known("cmd.exe").is_empty());
        assert!(known("wsl.exe").is_empty());
        assert!(registered("cmd.exe").is_empty());
        assert!(registered("wsl.exe").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn powershell_seven_is_looked_for_under_the_program_files_it_installs_into() {
        let candidates = known("pwsh");

        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.ends_with("pwsh.exe")),
            "{candidates:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_registry_names_where_windows_powershell_is() {
        let registered = registered("powershell.exe");

        let (program, version) = registered
            .first()
            .expect("every Windows machine records this");
        assert!(program.is_file(), "and it is really there: {program:?}");
        assert!(
            version
                .as_deref()
                .is_some_and(|version| version.starts_with('5')),
            "the registry says 5.x, which the file itself does not: {version:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_key_every_guide_recommends_is_read_without_being_relied_on() {
        for (program, _) in registered("pwsh.exe") {
            assert!(
                program.ends_with("pwsh.exe"),
                "whatever it records is a path to the program: {program:?}"
            );
        }
    }
}
