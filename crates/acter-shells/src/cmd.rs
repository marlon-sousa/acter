//! Adapter: `cmd.exe`'s shell integration — the prompt string that carries OSC 133
//! markers, and the declaration of how far those markers reach.
//!
//! **The whole injection is one environment variable.** `cmd.exe`'s `PROMPT` understands
//! `$e` as escape, so the markers ride inside the prompt string itself: no snippet to
//! source, no profile to edit, nothing to fail silently on a locked-down machine. DESIGN's
//! command-boundary section named this mechanism; B4.5 is where it was measured off a real
//! pseudoconsole and turned into code.
//!
//! **What it cannot carry is `C` and `D`.** `PROMPT` is evaluated when the prompt is drawn
//! and `cmd.exe` has no post-execution hook, so "output starts here" and "the command
//! ended with this code" have nowhere to come from without a third-party layer such as
//! Clink — not a dependency this product takes. Hence [`MARKERS`], and hence the tracker
//! synthesizing `C` from the echo instead.
//!
//! This is knowledge about a shell rather than about a transport, which is why it lives
//! here and not in `acter-transports`: cmd over a local pseudoconsole and cmd over SSH want
//! the same string.

use acter_core::ShellMarkers;

/// The `PROMPT` value that makes `cmd.exe` mark its own prompt region and command line.
///
/// Read back off a real pseudoconsole, byte for byte: `ESC ]133;A ESC \`, then the drawn
/// prompt, then `ESC ]133;B ESC \`. `$P$G` is cmd's own default — the current directory
/// and a `>` — so a session with this set looks exactly like one without it.
pub const PROMPT: &str = r"$e]133;A$e\$P$G$e]133;B$e\";

/// The environment one `cmd.exe` session is started with.
///
/// A slice rather than a map because there is one variable and the transport only iterates
/// it; a shell that needs more says so by returning more.
pub const ENVIRONMENT: &[(&str, &str)] = &[("PROMPT", PROMPT)];

/// How far cmd's markers reach: the prompt region and the command line, and nothing about
/// output or exit codes.
pub const MARKERS: ShellMarkers = ShellMarkers::PromptAndCommandLine;

/// Whether this program is `cmd.exe`, however it was named.
///
/// Case-insensitive and extension-insensitive, because `cmd`, `cmd.exe` and a full path to
/// it are all the same shell and a user typing any of them means the same thing. Anything
/// else is not claimed: a shell this crate does not know gets no injection and the full
/// marker assumption, which is exactly the unintegrated session it already had.
pub fn is_cmd(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    program.eq_ignore_ascii_case("cmd") || program.eq_ignore_ascii_case("cmd.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes matter more than the string does: `$e` is what cmd expands to escape, and
    /// a `PROMPT` missing either marker is a session that looks integrated and is not.
    #[test]
    fn the_prompt_carries_both_markers_around_cmds_own_default() {
        assert!(PROMPT.starts_with(r"$e]133;A$e\"));
        assert!(PROMPT.ends_with(r"$e]133;B$e\"));
        assert!(PROMPT.contains("$P$G"));
    }

    #[test]
    fn the_environment_sets_the_prompt_and_nothing_else() {
        assert_eq!(ENVIRONMENT, &[("PROMPT", PROMPT)]);
    }

    #[test]
    fn cmd_is_recognized_however_it_was_named() {
        for named in ["cmd", "cmd.exe", "CMD.EXE", r"C:\Windows\system32\cmd.exe"] {
            assert!(is_cmd(named), "{named} is cmd");
        }
    }

    #[test]
    fn nothing_else_is_claimed() {
        for named in [
            "powershell.exe",
            "pwsh",
            "bash",
            r"C:\bin\wsl.exe",
            "cmdlet.exe",
        ] {
            assert!(!is_cmd(named), "{named} is not cmd");
        }
    }
}
