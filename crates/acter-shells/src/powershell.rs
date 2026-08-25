//! Adapter: PowerShell behind the `ShellAdapter` port — both editions, the shell-integration
//! snippet they are started with, and the line that ends one.
//!
//! **The first shell that can tell Acter a command ended, and with what.** cmd's `PROMPT`
//! can mark where a prompt begins and where a command line begins and nothing more (B4.5),
//! so every session until now has closed its blocks by inference and has never had an exit
//! code. PowerShell has a hook for each of the four OSC 133 markers, which is what makes
//! this the shell where `CommandFinished` and its verdict stop being machinery built
//! against fixtures.
//!
//! **Everything below was read off a real pseudoconsole**, the way B4.5 read cmd's
//! `PROMPT`, and the measurement disagreed with the plan three times: PSReadLine turns
//! itself off in front of a screen reader and takes the obvious `C` hook with it,
//! `$LASTEXITCODE` reports the code of some *earlier* command unless it is checked for
//! staleness, and neither control byte that is supposed to mean end-of-input ends a
//! PowerShell session at all. Each of those is a decision below rather than a comment,
//! because each of them would otherwise have shipped as a defect nobody could see.

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

/// PowerShell as Acter starts it, in one of its two editions.
///
/// Carries the program name it was selected by rather than a constant, for [`Cmd`]'s
/// reason: `pwsh`, `pwsh.exe` and a full path to it are the same shell and which of them
/// reaches the transport is the user's business.
///
/// [`Cmd`]: crate::Cmd
pub struct PowerShell {
    program: String,
}

impl PowerShell {
    /// Windows PowerShell 5.1, which is on every Windows machine and cannot be removed.
    pub fn windows() -> Self {
        Self::new(WINDOWS)
    }

    /// PowerShell 7 or later, which is installed separately and may not be there.
    ///
    /// **Whether it is on this machine is not this type's question.** An adapter answers
    /// "how do I start this edition", which is a pure function and is answerable for an
    /// edition nobody installed; finding out what a machine has is I/O, and it belongs to
    /// the port B5.3 introduces and the list B7 builds (spec B5.2, decision 1).
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

    /// All four, and measured all four — the first shell Acter can say that about.
    fn markers(&self) -> ShellMarkers {
        MARKERS
    }

    fn eof(&self) -> Option<Vec<u8>> {
        Some(EOF.to_vec())
    }
}

/// Windows PowerShell 5.1's program name.
const WINDOWS: &str = "powershell.exe";

/// PowerShell 7's program name. A different executable rather than a different version of
/// the same one, which is why the two editions can be installed side by side and why the
/// program name is the only thing that differs between them here.
const SEVEN: &str = "pwsh.exe";

/// No copyright banner. Windows PowerShell prints four lines of one otherwise, and a
/// session that opens by reading a copyright notice aloud is four lines a listener has to
/// get past before they reach their prompt.
const NO_LOGO: &str = "-NoLogo";

/// Stay interactive after the injection has run. Without it PowerShell executes
/// [`SNIPPET`] and exits, which is a session that ends before it starts.
const NO_EXIT: &str = "-NoExit";

const COMMAND: &str = "-Command";

/// **The whole injection is one argument, and it writes nothing to the user's machine.**
///
/// The same stance cmd's `PROMPT` variable takes and for the same reasons: no profile
/// edited, no script dropped in a temp directory, nothing to clean up and nothing left
/// behind if Acter is killed. It also sidesteps execution policy, because a `-Command`
/// string is not a script file — **measured 2026-08-24**, a Windows PowerShell 5.1 session
/// started with `-ExecutionPolicy Restricted` runs this and emits every marker. (PowerShell
/// 7 under `-ExecutionPolicy AllSigned` stops at a publisher-trust prompt before reaching
/// this string at all, because the host auto-imports PSReadLine's own `.ps1xml` first. That
/// is the host's behaviour in any terminal and not something an injection can avoid.)
///
/// What each piece does, in the order it appears:
///
/// **It removes PSReadLine.** This is the change the measurement forced and it is not
/// incidental — see [`the reason`](self#psreadline). `-ErrorAction SilentlyContinue`
/// because on the edition where it matters most the module is not loaded to begin with.
///
/// **The prompt function carries `D`, `A` and `B`.** PowerShell runs `prompt` after every
/// command, so the marker that says the *last* command ended rides on the front of the
/// *next* prompt, which is why `D` comes first and why it is suppressed until a command
/// has actually run. `A` and `B` then bracket the prompt the way they do in cmd.
///
/// **The original prompt is called, not replaced.** Whatever the user's profile drew is
/// what they still hear; this only wraps it. A profile whose prompt throws is caught, and
/// a session with a plain `PS>` is better than one that cannot draw a prompt at all.
///
/// **`C` comes from a command-lookup hook**, which is the one piece with no obvious home —
/// see [`the reason`](self#the-output-marker).
///
/// The exit-code rule is [`its own section`](self#the-exit-code).
///
/// # PSReadLine
///
/// **Measured 2026-08-24 on a machine running NVDA.** Windows PowerShell 5.1 starts by
/// saying `Warning: PowerShell detected that you might be using a screen reader and has
/// disabled PSReadLine for compatibility purposes`, and it means it: `PSConsoleHostReadLine`
/// does not exist in that session, so the hook every other terminal's PowerShell
/// integration hangs `C` on is not there. PowerShell 7 makes no such check and leaves
/// PSReadLine on.
///
/// So the two editions disagreed about the *shape of the byte stream* in front of exactly
/// the users this product is for, and with PSReadLine active the result was not merely
/// different, it was wrong: `C` arrived immediately after `B`, before the user's line had
/// been read, which makes the echoed command line block content instead of the block's
/// heading. PSReadLine also redraws the row as it goes — a submitted `echo acter-hello`
/// came back as `ESC[?25l echo acter-hel ESC[?25h lo` — which is the rewritten row B6.1
/// was written for and pure noise to a listener.
///
/// Removing it makes both editions produce the same stream, which is the same decision
/// Microsoft already made for 5.1 in front of a screen reader. Acter loses nothing by it:
/// the edit field, the history and the completion are Acter's own, so PowerShell's line
/// editor is never the thing a user types into. It is per session and per process —
/// `Import-Module PSReadLine` brings it straight back — and nothing on disk is touched.
///
/// # The output marker
///
/// `C` means "the command line has been read and output starts here", so it cannot come
/// from the prompt function, which has already returned by then. With PSReadLine gone
/// there is no readline function to wrap either, and defining one means owning the line
/// editor: measured, a `PSConsoleHostReadLine` built on `[Console]::ReadLine()` does work
/// and does turn Ctrl+Z into a real end of input, but it also emits a stray `C` every time
/// an interrupt breaks the read, which opens a block with no command line in it.
///
/// What is left is `PreCommandLookupAction`, a scriptblock PowerShell runs when it resolves
/// a command name — after the line is read, before anything runs. A flag the prompt arms
/// and the hook disarms keeps it to one `C` per command however many names that command
/// resolves.
///
/// **The case this had to survive is a line that looks up no command at all.** `1..3`,
/// `2+2` and `'a literal string'` all produce output and none of them is a command
/// invocation, and had the hook not fired their output would have landed in the region
/// between `B` and `C` — which `Pump::wants` excludes, so the user would have heard
/// nothing. Measured on both editions: every one of them emits `C` before its first line
/// of output, because PowerShell resolves the formatting pipeline as a command. A line
/// that produces no output at all still gets one, from the next prompt's own lookup, which
/// is an empty output region and is exactly right.
///
/// # The exit code
///
/// `$?` says whether the last command succeeded and `$LASTEXITCODE` says what a *native*
/// program exited with, and a rule that reads either one alone is wrong. `$?` alone
/// throws away the code the user wants; `$LASTEXITCODE` alone is worse, because it is
/// **stale**: measured, `cmd /c exit 3` followed by a failing `Get-Item` reported
/// `D;3` for the `Get-Item`, naming an exit code that belonged to the command before it.
///
/// The rule is therefore: success is 0; a failure whose `$LASTEXITCODE` *changed* since
/// the last prompt reports that code, because only a native program could have changed it;
/// any other failure reports 1. Measured across both editions: `cmd /c exit 3` gives
/// `D;3`, a failing cmdlet after it gives `D;1`, a successful command gives `D;0`, and an
/// interrupted command gives `D;-1073741510`, which is the Windows control-C exit status
/// and an honest report that the command did not finish.
///
/// **What the rule rounds, said out loud:** the same native exit code twice in a row is
/// reported as 3 and then as 1, because nothing distinguishes an unchanged
/// `$LASTEXITCODE` from a cmdlet failing behind it. It never turns a failure into a
/// success or a success into a failure — which is the direction that matters, since a
/// wrong verdict is spoken about a command that did the opposite — and the alternative
/// was clearing `$LASTEXITCODE` behind the user's back, which would have broken the
/// variable for anyone who reads it.
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

/// What ends a PowerShell session: the line `exit`, with the carriage return that submits
/// it.
///
/// **Not a control byte, and the spec said it would be.** B5.2 was written expecting
/// Ctrl+Z, on the reasoning that it is what a Windows console means by end of input.
/// Measured 2026-08-24 against both editions on a real pseudoconsole, neither `0x1a` nor
/// `0x04` ends a session: alone, each is echoed as caret text and nothing happens; followed
/// by Enter, the caret text is submitted as a command line and PowerShell answers that
/// `␦` is not the name of a cmdlet — which is worse than doing nothing, because the user
/// hears an error about something they never typed. `exit` followed by a carriage return
/// closes both editions cleanly, the read channel ends, and the session ends the way the
/// transport models an ending.
///
/// It is a whole line rather than a byte, and that is why
/// [`ShellAdapter::eof`](acter_core::ShellAdapter::eof) answers in bytes: a port shaped
/// around "which control character" could not have expressed the answer for the first
/// shell that needed it.
pub(crate) const EOF: &[u8] = b"exit\r";

/// All four markers, because [`SNIPPET`] has a hook for each and every one of them was
/// seen arriving.
///
/// The difference from cmd is the whole point of this entry: `Full` is what lets the
/// boundary tracker close a block on a `D` that really says the command ended, with the
/// code it ended with, instead of inferring an ending from the next prompt.
pub(crate) const MARKERS: ShellMarkers = ShellMarkers::Full;

/// Whether this program is PowerShell, in either edition and however it was named.
///
/// Case-insensitive and path-insensitive for [`is_cmd`](crate::cmd::is_cmd)'s reason. Both
/// editions answer the same predicate because both are started the same way: the snippet,
/// the arguments, the markers and the end-of-input line are identical, and the program
/// name is the only thing that differs (spec B5.2, decision 1).
pub(crate) fn is_powershell(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    ["powershell", "powershell.exe", "pwsh", "pwsh.exe"]
        .iter()
        .any(|known| program.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two editions differ in exactly one thing, and pinning that is what stops a
    /// future edition-specific tweak from being made silently: the moment they differ in
    /// anything else, one of them has stopped being measured.
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

    /// An edition that is not installed still answers, which is what makes this a value
    /// rather than a discovery: B7's connect list has to be able to name PowerShell 7 and
    /// find out separately whether this machine has it (spec B5.2, decision 1).
    #[test]
    fn an_edition_answers_whether_or_not_the_machine_has_it() {
        let launch = PowerShell::seven().launch();

        assert_eq!(launch.program, "pwsh.exe");
        assert!(launch.args.contains(&"-Command".to_owned()));
    }

    /// The injection is an argument and nothing else, which is the promise that nothing is
    /// written to the user's machine: no variable to set, no file to drop, and a session
    /// that leaves no trace when Acter is killed.
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

    /// A hook per marker. The bytes are asserted in the shape the snippet builds them —
    /// `$e` is an escape and the terminator is `ESC \` — because a snippet that had lost
    /// one of these would still start a shell and would produce a session whose blocks are
    /// silently wrong.
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

    /// **The regression this would hide is silent**, which is why it is pinned as its own
    /// fact rather than left to the string above: with PSReadLine active the output marker
    /// arrives before the user's line has been read, and every block in the session gets
    /// its heading wrong. Measured on PowerShell 7, where PSReadLine is on by default.
    #[test]
    fn the_snippet_takes_psreadline_out_of_the_session() {
        assert!(SNIPPET.starts_with("Remove-Module PSReadLine "));
        assert!(
            SNIPPET.contains("-ErrorAction SilentlyContinue"),
            "and says nothing when it was not loaded, which is the ordinary case in \
             Windows PowerShell in front of a screen reader"
        );
    }

    /// The exit-code rule, pinned as the three things it must weigh: whether PowerShell
    /// itself considered the command successful, what a native program exited with, and
    /// whether that code is this command's or the one before it's.
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

    /// The user's own prompt is wrapped rather than replaced, so a session started by
    /// Acter looks like the one the user configured.
    #[test]
    fn the_prompt_the_user_configured_is_the_prompt_they_hear() {
        assert!(SNIPPET.contains("$global:__acterPrompt = $function:prompt"));
        assert!(SNIPPET.contains("& $global:__acterPrompt"));
        assert!(
            SNIPPET.contains("catch { $body = 'PS> ' }"),
            "and a profile whose prompt throws still leaves a usable session"
        );
    }

    /// End of input is a line, not a keystroke — the fact that decided the port's shape.
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

    /// A shell selected by a full path is started by that path, because a user who named
    /// one edition's executable meant that edition and not whichever one is first on
    /// `PATH`.
    #[test]
    fn the_launch_carries_the_program_it_was_named_by() {
        let named = r"C:\Program Files\PowerShell\7\pwsh.exe";

        assert_eq!(PowerShell::new(named).launch().program, named);
    }
}
