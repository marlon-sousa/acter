//! Adapter: `cmd.exe` behind the `ShellAdapter` port — how the shell is started, the
//! prompt string that carries OSC 133 markers, and the declaration of how far those
//! markers reach.
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

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

/// `cmd.exe` as Acter starts it.
///
/// Carries the program name it was selected by rather than a constant, because `cmd`,
/// `cmd.exe` and a full path to it are the same shell and which of them reaches the
/// transport is the user's business, not this crate's.
pub struct Cmd {
    program: String,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

/// What `cmd.exe`'s line editor reads as "discard whatever is pending on this line".
///
/// Measured, not read: with it a submitted line runs as itself and the caret text ConPTY left
/// on the row is erased; without it the shell answers about a command the user never typed
/// (spec B4.5, decision 7).
const ESCAPE: u8 = 0x1b;

impl ShellAdapter for Cmd {
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: ARGS.iter().map(|arg| (*arg).to_owned()).collect(),
            environment: ENVIRONMENT
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn markers(&self) -> ShellMarkers {
        MARKERS
    }

    /// Nobody has measured what ends a `cmd.exe` session, so this says so.
    ///
    /// It is a gap rather than a claim: B5.2 measured PowerShell's answer and had no
    /// reason to spawn a `cmd.exe` to find out about this one, and answering with a byte
    /// that looked right in documentation is precisely the habit that spec exists to
    /// break. A `Ctrl+D` in a cmd session therefore reports that there was nothing to act
    /// on, which is true.
    /// **Escape, and `cmd.exe` is the only shell this is measured for** (spec B4.5,
    /// decision 7). Its line editor treats escape as "discard the pending line", which is what
    /// lets a submitted line survive the cursor-position answer ConPTY leaves in front of it —
    /// measured both ways: without it the shell answers `'s not recognized as an internal or
    /// external command,`, naming a command the user never typed.
    ///
    /// **It is this adapter's answer rather than an inference from the marker claim, since
    /// B9.5.** The session used to read `PromptAndCommandLine` as "this is cmd"; POSIX `sh`
    /// made that claim too for a day, and a POSIX reader takes an escape as a meta prefix
    /// rather than as a discard. `sh` has since moved on to a claim of its own (roadmap
    /// 23.15), which leaves cmd alone here again — and the question stays the adapter's,
    /// because what a line editor does with a byte was never what a marker claim was about.
    fn discards_line(&self) -> Option<u8> {
        Some(ESCAPE)
    }

    fn eof(&self) -> Option<Vec<u8>> {
        None
    }
}

/// The `PROMPT` value that makes `cmd.exe` mark its own prompt region and command line.
///
/// Read back off a real pseudoconsole, byte for byte: `ESC ]133;A ESC \`, then the drawn
/// prompt, then `ESC ]133;B ESC \`. `$P$G` is cmd's own default — the current directory
/// and a `>` — so a session with this set looks exactly like one without it.
pub(crate) const PROMPT: &str = r"$e]133;A$e\$P$G$e]133;B$e\";

/// The environment one `cmd.exe` session is started with.
///
/// A slice rather than a map because there is one variable and the transport only iterates
/// it; a shell that needs more says so by returning more.
pub(crate) const ENVIRONMENT: &[(&str, &str)] = &[("PROMPT", PROMPT)];

/// The arguments one `cmd.exe` session is started with.
///
/// `/Q` turns off cmd's own script-style echo of the commands it runs, so the only echo
/// in the stream is the pseudoconsole's echo of what was typed — which is what B4.9's
/// "no command line is read back" and B6.1's echo-based correlation were measured
/// against. `/K` keeps the shell running after each command.
///
/// **They live here so that the application and the suites cannot measure different
/// streams.** Until B5.1 the tests spelled them out and the application spawned `cmd.exe`
/// bare, so cmd's own echo was off in every measurement and on in the product (spec B5.1,
/// decision 5).
pub(crate) const ARGS: &[&str] = &["/Q", "/K"];

/// How far cmd's markers reach: the prompt region and the command line, and nothing about
/// output or exit codes.
pub(crate) const MARKERS: ShellMarkers = ShellMarkers::PromptAndCommandLine;

/// Whether this program is `cmd.exe`, however it was named.
///
/// Case-insensitive and extension-insensitive, because `cmd`, `cmd.exe` and a full path to
/// it are all the same shell and a user typing any of them means the same thing. Anything
/// else is not claimed: a shell this crate does not know gets no injection and the full
/// marker assumption, which is exactly the unintegrated session it already had.
pub(crate) fn is_cmd(program: &str) -> bool {
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
    fn the_launch_carries_the_program_it_was_named_by_with_the_arguments_and_the_injection() {
        let launch = Cmd::new(r"C:\Windows\system32\cmd.exe").launch();

        assert_eq!(launch.program, r"C:\Windows\system32\cmd.exe");
        assert_eq!(launch.args, ["/Q", "/K"]);
        assert_eq!(
            launch.environment,
            [("PROMPT".to_owned(), PROMPT.to_owned())]
        );
    }

    #[test]
    fn cmd_claims_its_prompt_and_command_line_and_no_more() {
        assert_eq!(
            Cmd::new("cmd.exe").markers(),
            ShellMarkers::PromptAndCommandLine
        );
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
