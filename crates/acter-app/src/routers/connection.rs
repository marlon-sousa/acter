//! Router: what the far end of this session is called, for the window to name itself.
//!
//! **Not the catalogue's label yet, and that is a stated debt.** B5.4 gives every connection
//! kind a display name — "Windows PowerShell", "WSL: Ubuntu" — chosen for a listener reading
//! a connect list, and once that catalogue reaches a running session this command hands the
//! same string back, so what a user chose and what the window calls itself are the same
//! words. Until then it answers what the launch actually named, which is honest and
//! duplicates nothing: a second copy of those labels here would be two places to change and
//! one of them would be forgotten (spec A9).

use std::env;

/// The environment variable naming the shell, restated from the container rather than
/// shared: this is a router answering a question, and reaching into the composition root
/// for a constant would be the wrong direction.
const SHELL_ENV: &str = "ACTER_SHELL";

/// What this window is connected to, or `None` for the scripted session — which is not a
/// far end anybody chose and has no name worth putting in a title bar.
#[tauri::command]
pub(crate) fn connection() -> Option<String> {
    env::var(SHELL_ENV).ok().map(|program| named(&program))
}

/// The program as the user named it, without the extension a title bar gains nothing from.
///
/// Path and case are left alone: somebody who launched a specific `pwsh.exe` by full path is
/// telling us which one they meant, and a title that quietly renamed it would be answering a
/// question they did not ask.
fn named(program: &str) -> String {
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    file.strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".EXE"))
        .unwrap_or(file)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_is_named_by_its_program_without_the_extension() {
        assert_eq!(named("powershell.exe"), "powershell");
        assert_eq!(named("pwsh"), "pwsh");
        assert_eq!(named(r"C:\Windows\system32\cmd.exe"), "cmd");
    }

    /// A name with no extension at all survives, and so does one whose "extension" is part
    /// of the name.
    #[test]
    fn nothing_else_is_trimmed() {
        assert_eq!(named("wsl"), "wsl");
        assert_eq!(named("my.shell"), "my.shell");
    }
}
