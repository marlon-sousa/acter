//! Adapter: the places a shell is installed that `PATH` does not necessarily name.
//!
//! **`PATH` alone is where the user's proposal breaks** (spec B5.7, decision 2). An MSI
//! install with "add to PATH" unchecked is invisible to it; scoop and chocolatey put *shims*
//! on it that are not the program. Windows Terminal answers this by dropping `PATH` entirely
//! and enumerating known roots and Store package identities — which is right about everything
//! except what the name means to this user, so here the two are unioned instead.
//!
//! **The Store is reached through its execution alias rather than through `PackageManager`.**
//! Windows Terminal calls `PackageManager.FindPackagesForUser()`, which is WinRT and needs an
//! apartment, a projection and a good deal more of Windows than anything else in this
//! product touches. The alias resolution decision 4 already requires yields the same two
//! facts — the package family and the file — from a reparse point, and measured 2026-08-27
//! the package directory is on `PATH` here in its own right. So the Store arrives through
//! `PATH` and through the alias, and this file does not go looking for it a third way.
//!
//! **The registry is right for Windows PowerShell and wrong as a general rule** (decision 2).
//! Measured 2026-08-27: `HKLM\SOFTWARE\Microsoft\PowerShell\3\PowerShellEngine` exists and
//! reports 5.1.26100.8875, and `HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions`
//! **does not exist at all** on a machine whose PowerShell 7 came from the Store. Microsoft's
//! own detection guidance is from 2009, covers PowerShell 1.0, and warns against depending on
//! any other registry key — so both are read, and neither is believed to be the whole answer.

use std::path::{Path, PathBuf};

use super::{POWERSHELL, environment};

/// The two program names this file knows anything about. Every other program is `PATH`'s
/// business alone, which is the honest answer: there is no known root for `cmd.exe` beyond
/// the one Windows itself guarantees.
const PWSH: &str = "pwsh";
const WINDOWS_POWERSHELL: &str = "powershell";

/// The environment variables naming the places an installer puts PowerShell 7, including the
/// x86 and ARM twins — because a 64-bit Acter reading only `%ProgramFiles%` would miss a
/// 32-bit install and vice versa.
const PROGRAM_FILES: [&str; 3] = ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432"];

/// Where a dotnet tool install puts it, which Windows Terminal also looks in — and which
/// says nothing about its version, so it is one of the installs decision 3 reports as
/// indeterminable.
const DOTNET_TOOLS: [&str; 2] = [".dotnet", "tools"];

/// Where Windows PowerShell lives, under the system directory.
const WINDOWS_POWERSHELL_DIRECTORY: [&str; 2] = ["WindowsPowerShell", "v1.0"];

/// Every file this program might be, in places nobody had to put on `PATH`.
///
/// Candidates rather than installs: whether each one is really there is the caller's
/// question, asked once for every source.
pub(super) fn known(program: &str) -> Vec<PathBuf> {
    let file = file_name(program);
    match stem(program).as_str() {
        PWSH => powershell_seven(&file),
        WINDOWS_POWERSHELL => windows_powershell(&file),
        _ => Vec::new(),
    }
}

/// The versioned install directories, and the dotnet tools directory.
fn powershell_seven(file: &str) -> Vec<PathBuf> {
    let environment = environment();
    let mut candidates = Vec::new();
    for name in PROGRAM_FILES {
        let Some(root) = at(&environment, name) else {
            continue;
        };
        // Every child of `…\PowerShell`, because the directory name *is* the version
        // (decision 3) and this product has no list of which versions exist.
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

/// The one place the edition that ships with Windows is, spelled out rather than left to
/// `PATH` — a machine whose `PATH` has been trimmed still has it.
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

/// Every install the registry records, with the version it records for it.
///
/// **Both keys, and neither trusted as the whole answer.** The `PowerShellCore` key is what
/// every guide recommends and is absent on the machine this was written on; the
/// `PowerShellEngine` key is the one that answers for the edition Windows ships.
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

/// The key everybody recommends, which **did not exist** on the machine this was written on
/// (decision 2) — so this reads it and expects nothing.
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

/// The value of a variable, or `None` for a machine that does not set it — the x86 twin does
/// not exist on every machine, and neither does a home directory.
fn at(environment: &[(String, PathBuf)], name: &str) -> Option<PathBuf> {
    environment
        .iter()
        .find(|(named, _)| named == name)
        .map(|(_, value)| value.clone())
}

/// The program as a filename, so a name a user typed without its extension still finds the
/// file a known root holds.
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

/// The program's bare name, lower-cased, which is what decides whether this file knows
/// anywhere to look.
fn stem(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Reading the two keys, and nothing else — the smallest registry this product needs.
#[cfg(windows)]
mod registry {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegGetValueW, RegOpenKeyExW,
    };

    /// Long enough for any key name Windows allows, which is 255 characters.
    const NAME: usize = 256;

    /// A string value under `HKEY_LOCAL_MACHINE`, or `None` for a key or value that is not
    /// there — which for `PowerShellCore\InstalledVersions` is the ordinary case.
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

    /// The names of a key's subkeys, and an empty list for a key that is not there.
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

    /// A name a user typed without its extension still names the file a known root holds —
    /// which is the same rule `PATHEXT` applies to `PATH`, applied where `PATHEXT` does not
    /// reach.
    #[test]
    fn a_name_with_no_extension_still_names_a_file() {
        assert_eq!(file_name("pwsh"), "pwsh.exe");
        assert_eq!(file_name("pwsh.exe"), "pwsh.exe");
        // **Joined rather than written out** (M1): `Path` splits on the host's separator, so a
        // backslash path is one filename off Windows and this asserted the whole string.
        let full: PathBuf = ["tools", "pwsh", "pwsh.exe"].iter().collect();
        assert_eq!(file_name(&full.display().to_string()), "pwsh.exe");
    }

    /// **Only two programs have anywhere else to be looked for**, and saying so is the honest
    /// answer: there is no known root for `cmd.exe` beyond the one Windows guarantees, and
    /// inventing one would be a place to find a `cmd.exe` nobody installed.
    #[test]
    fn a_program_with_nowhere_else_to_be_has_no_known_roots() {
        assert!(known("cmd.exe").is_empty());
        assert!(known("wsl.exe").is_empty());
        assert!(registered("cmd.exe").is_empty());
        assert!(registered("wsl.exe").is_empty());
    }

    /// The versioned install directories are looked in by *name*, so an install of a version
    /// this product has never heard of is still found — which is what decision 3 means by
    /// taking the version from the directory.
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

    /// **The measured registry** (decision 2): the key that answers for the edition Windows
    /// ships is read, and what it names is where that edition really is.
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

    /// **And the key everybody recommends is not believed to exist** (decision 2). This asks
    /// for it and asserts only that asking is safe — on the machine this was written on it is
    /// absent, and on a machine with an MSI install it is not.
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
