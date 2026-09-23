//! Adapter: PowerShell behind the `ShellAdapter` port — both editions, the shell-integration
//! snippet they are started with, and the line that ends one.

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

pub struct PowerShell {
    program: String,
}

impl PowerShell {
    pub fn windows() -> Self {
        Self::new(WINDOWS)
    }

    pub fn seven() -> Self {
        Self::new(SEVEN)
    }

    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl ShellAdapter for PowerShell {
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: [NO_LOGO, NO_EXIT, COMMAND, SNIPPET]
                .iter()
                .map(|arg| (*arg).to_owned())
                .collect(),
            environment: Vec::new(),
        }
    }

    fn markers(&self) -> ShellMarkers {
        MARKERS
    }

    fn eof(&self) -> Option<Vec<u8>> {
        Some(EOF.to_vec())
    }
}

const WINDOWS: &str = "powershell.exe";

const SEVEN: &str = "pwsh.exe";

const NO_LOGO: &str = "-NoLogo";

const NO_EXIT: &str = "-NoExit";

const COMMAND: &str = "-Command";

/// Windows PowerShell 5.1 under `-ExecutionPolicy Restricted` runs this string and emits every
/// marker; PowerShell 7 under `-ExecutionPolicy AllSigned` stops at a publisher-trust prompt
/// for PSReadLine's `.ps1xml` before reaching it.
///
/// Windows PowerShell 5.1 disables PSReadLine in front of a screen reader and PowerShell 7 does
/// not; with PSReadLine loaded, `C` arrives before the typed line is read.
///
/// In both editions `PreCommandLookupAction` also fires for a line that names no command, such
/// as `1..3` or `2+2`, before its first line of output.
///
/// In both editions `$LASTEXITCODE` is stale after a failing cmdlet (`cmd /c exit 3` then a
/// failing `Get-Item` still reads 3), so a code counts only when it changed and the same native
/// code twice in a row reports 1 the second time.
pub(crate) const SNIPPET: &str = concat!(
    "Remove-Module PSReadLine -Force -ErrorAction SilentlyContinue; ",
    "$global:__acterOutput = $true; ",
    "$global:__acterRan = $false; ",
    "$global:__acterPrompt = $function:prompt; ",
    "function global:prompt { ",
    "$ok = $?; $code = $LASTEXITCODE; $e = [char]27; $out = ''; ",
    "if ($global:__acterRan) { $x = 0; ",
    "if (-not $ok) { if (($code -is [int]) -and ($code -ne 0) -and ",
    "($code -ne $global:__acterCode)) { $x = $code } else { $x = 1 } }; ",
    "$out = $out + $e + ']133;D;' + $x + $e + '\\' }; ",
    "$global:__acterCode = $code; $global:__acterRan = $true; $global:__acterOutput = $true; ",
    "$body = ''; ",
    "try { $body = [string](& $global:__acterPrompt) } catch { $body = 'PS> ' }; ",
    "$global:__acterOutput = $false; ",
    "$out + $e + ']133;A' + $e + '\\' + $body + $e + ']133;B' + $e + '\\' }; ",
    "$ExecutionContext.InvokeCommand.PreCommandLookupAction = { ",
    "if (-not $global:__acterOutput) { $global:__acterOutput = $true; ",
    "[Console]::Write([char]27 + ']133;C' + [char]27 + '\\') } }"
);

/// In both editions `0x1a` and `0x04` are echoed as caret text and end nothing; `exit` and a
/// carriage return close the session.
pub(crate) const EOF: &[u8] = b"exit\r";

pub(crate) const MARKERS: ShellMarkers = ShellMarkers::Full;

pub(crate) fn is_powershell(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    ["powershell", "powershell.exe", "pwsh", "pwsh.exe"]
        .iter()
        .any(|known| program.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_editions_differ_only_in_which_program_is_started() {
        let windows = PowerShell::windows().launch();
        let seven = PowerShell::seven().launch();

        assert_eq!(windows.program, "powershell.exe");
        assert_eq!(seven.program, "pwsh.exe");
        assert_eq!(windows.args, seven.args);
        assert_eq!(windows.environment, seven.environment);
        assert_eq!(
            PowerShell::windows().markers(),
            PowerShell::seven().markers()
        );
        assert_eq!(PowerShell::windows().eof(), PowerShell::seven().eof());
    }

    #[test]
    fn an_edition_answers_whether_or_not_the_machine_has_it() {
        let launch = PowerShell::seven().launch();

        assert_eq!(launch.program, "pwsh.exe");
        assert!(launch.args.contains(&"-Command".to_owned()));
    }

    #[test]
    fn the_whole_injection_is_one_argument_and_no_environment_at_all() {
        let launch = PowerShell::windows().launch();

        assert_eq!(launch.args[..3], ["-NoLogo", "-NoExit", "-Command"]);
        assert_eq!(launch.args.len(), 4);
        assert_eq!(launch.args[3], SNIPPET);
        assert!(
            launch.environment.is_empty(),
            "nothing is injected through the environment"
        );
    }

    #[test]
    fn the_snippet_has_a_hook_for_every_one_of_the_four_markers() {
        assert!(SNIPPET.contains(r"']133;A' + $e + '\'"), "prompt start");
        assert!(
            SNIPPET.contains(r"']133;B' + $e + '\'"),
            "command line start"
        );
        assert!(
            SNIPPET.contains(r"']133;C' + [char]27 + '\'"),
            "output start"
        );
        assert!(
            SNIPPET.contains(r"']133;D;' + $x + $e + '\'"),
            "command end"
        );
    }

    #[test]
    fn the_snippet_takes_psreadline_out_of_the_session() {
        assert!(SNIPPET.starts_with("Remove-Module PSReadLine "));
        assert!(
            SNIPPET.contains("-ErrorAction SilentlyContinue"),
            "and says nothing when it was not loaded, which is the ordinary case in \
             Windows PowerShell in front of a screen reader"
        );
    }

    #[test]
    fn the_exit_code_is_weighed_against_staleness_and_not_read_off_one_variable() {
        assert!(SNIPPET.contains("$ok = $?"), "PowerShell's own verdict");
        assert!(
            SNIPPET.contains("$code = $LASTEXITCODE"),
            "and a native program's code"
        );
        assert!(
            SNIPPET.contains("($code -ne $global:__acterCode)"),
            "used only when it changed, so a failing cmdlet cannot inherit the exit code \
             of the native program before it"
        );
    }

    #[test]
    fn the_prompt_the_user_configured_is_the_prompt_they_hear() {
        assert!(SNIPPET.contains("$global:__acterPrompt = $function:prompt"));
        assert!(SNIPPET.contains("& $global:__acterPrompt"));
        assert!(
            SNIPPET.contains("catch { $body = 'PS> ' }"),
            "and a profile whose prompt throws still leaves a usable session"
        );
    }

    #[test]
    fn ending_the_session_is_the_line_exit_and_not_a_control_byte() {
        assert_eq!(PowerShell::windows().eof(), Some(b"exit\r".to_vec()));
        let eof = PowerShell::windows()
            .eof()
            .expect("PowerShell has an answer");
        assert!(
            !eof.contains(&0x1a) && !eof.contains(&0x04),
            "neither control byte ends a PowerShell session, measured on both editions"
        );
    }

    #[test]
    fn powershell_claims_every_marker_a_shell_can_emit() {
        assert_eq!(PowerShell::windows().markers(), ShellMarkers::Full);
    }

    #[test]
    fn powershell_is_recognized_in_either_edition_however_it_was_named() {
        for named in [
            "powershell",
            "powershell.exe",
            "POWERSHELL.EXE",
            "pwsh",
            "pwsh.exe",
            "PWSH",
            r"C:\Windows\system32\WindowsPowerShell\v1.0\powershell.exe",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
        ] {
            assert!(is_powershell(named), "{named} is PowerShell");
        }
    }

    #[test]
    fn nothing_else_is_claimed() {
        for named in [
            "cmd.exe",
            "bash",
            "wsl.exe",
            "powershellish.exe",
            "pwshx",
            "nushell.exe",
        ] {
            assert!(!is_powershell(named), "{named} is not PowerShell");
        }
    }

    #[test]
    fn the_launch_carries_the_program_it_was_named_by() {
        let named = r"C:\Program Files\PowerShell\7\pwsh.exe";

        assert_eq!(PowerShell::new(named).launch().program, named);
    }
}
