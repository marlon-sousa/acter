//! Entity/value: the bash program Acter sets `PROMPT_COMMAND` to, and the `WSLENV` entry
//! that carries it across the kernel boundary.
//!
//! **Measured off a real pseudoconsole on 2026-08-24, never taken from documentation**,
//! the way B4.5 measured cmd's `PROMPT`. Ubuntu 24.04.2 and Debian under WSL 2.5.7.0,
//! bash 5.2.21, driven through `LocalPty`. What three submissions produced, with the
//! escapes written out:
//!
//! - `ESC ] 133 ; A ESC \` then the distribution's own coloured prompt then
//!   `ESC ] 133 ; B ESC \`, then the echo of the submitted line;
//! - `ESC ] 133 ; C ESC \` then the command's output;
//! - `ESC ] 133 ; D ; 0 ESC \` after `echo`, `ESC ] 133 ; D ; 1 ESC \` after `false`, and
//!   `ESC ] 133 ; D ; 130 ESC \` after an interrupt.
//!
//! Counted over that session: four prompts drawn, four `A`, four `B`, three `C`, three
//! `D`. **The full OSC 133 cycle with real exit codes**, which is why WSL is the first
//! shipped shell whose adapter claims [`ShellMarkers::Full`](acter_core::ShellMarkers).
//!
//! ## Why this shape and not a simpler one
//!
//! **Nothing is written into the distribution** (spec B5.3, decision 2). The whole program
//! travels as an environment variable, so there is no snippet to source, no dotfile to
//! append and nothing left behind on a machine the user has to live in after Acter closes.
//! Verified the same day by hashing the home directory and `.bashrc` before and after an
//! integrated session, from outside and from inside it: byte-identical, and the directory
//! listing unchanged at thirty-one entries.
//!
//! **`PROMPT_COMMAND` rather than `PS1`**, which was the candidate order the spec set.
//! `PS1` loses: every Debian-family default `.bashrc` sets it unconditionally, so ours
//! would be overwritten before the first prompt. `PROMPT_COMMAND` wins for the mirror-image
//! reason — bash exports the environment into the shell *before* it sources `.bashrc`, and
//! that file leaves `PROMPT_COMMAND` alone. So the injection runs after the user's
//! configuration and gets to *wrap* the prompt that file chose, which is what the
//! measurement shows: Ubuntu's green-and-blue `marlon@splyt:/mnt/c/…$` is still there,
//! between our `A` and our `B`.
//!
//! **`PS1` is wrapped once, not rebuilt.** The user's prompt is theirs; all this adds is a
//! zero-width marker at each end, inside `\[` and `\]` so readline still counts the line's
//! printing width correctly and long lines wrap where they always did.
//!
//! ## The two bash facts this is built on, both measured rather than read
//!
//! **`C` needs a `DEBUG` trap, and the trap cannot disarm itself.** `PROMPT_COMMAND` runs
//! before a prompt, so it can say `D` and `A`, and `PS1` can say `B` — but nothing except
//! the `DEBUG` trap fires between the user pressing Enter and their command running, which
//! is exactly where `C` belongs. The obvious way to fire it once is for the trap to remove
//! itself, and on 2026-08-24 that was measured not to work: a `trap - DEBUG` executed
//! inside a shell function is discarded when the function returns (bash restores the
//! saved trap when `functrace` is off), while a `trap ... DEBUG` set inside one sticks.
//! The version built on self-removal emitted four `C` markers per command instead of one.
//! So the guard here is a variable, `__acter_seen`, set by the trap and cleared by the prompt.
//!
//! **A trap armed inside a function fires for the rest of that function.** Arming the trap
//! and *then* clearing the guard produced one spurious `C` before the very first prompt,
//! because the trap fired on the clearing statement itself. Arming last is what removes it:
//! in the final measurement the session's first byte of OSC 133 is an `A`.
//!
//! ## What is not claimed
//!
//! A `.bashrc` that sets `PROMPT_COMMAND` itself overwrites this and no marker is ever
//! emitted. That is not detectable from the Windows side and is not guessed at: such a
//! session simply never becomes integrated, the pacing policy flags it once the grace
//! period expires, and the user hears the existing explanation for an unintegrated
//! session rather than silence (spec B5.3, decision 3).

/// The `WSLENV` entry that carries the injection into the distribution.
///
/// **Nothing crosses the kernel boundary without it**, measured by 22.6 and re-confirmed
/// here: a variable set on the Windows side is simply absent inside the distribution
/// unless `WSLENV` names it. A bare name with no flags means "pass the value through
/// unchanged", which is what a shell program wants — the path-translation flags would
/// mangle it.
pub(crate) const WSLENV: (&str, &str) = ("WSLENV", "PROMPT_COMMAND");

/// The bash program `PROMPT_COMMAND` is set to.
///
/// One line, because it is an environment variable. Read it as four statements: capture
/// the exit status before anything else can overwrite it, define the `DEBUG` trap
/// handler, define the prompt handler, then run the prompt handler.
///
/// `__acter_status=$?` is first and matters: a function *definition* succeeds, so defining
/// the two handlers would otherwise reset `$?` to zero and every command would be reported
/// as having succeeded. The `DEBUG` trap running before this line does not disturb it —
/// measured, since `false` still reported `D;1`.
pub(crate) const PROMPT_COMMAND: &str = concat!(
    "__acter_status=$?; ",
    // Output starts here. The guard makes this the *first* simple command of whatever the
    // user submitted rather than every one of them: a pipeline or a loop fires the trap
    // repeatedly and only the first firing means "the command began".
    "__acter_output() { if [ -n \"$__acter_seen\" ]; then return; fi; __acter_seen=1; ",
    "printf '\\033]133;C\\033\\\\'; }; ",
    "__acter_prompt() { ",
    // `D` closes the command that just ended, so it is skipped before the first one: a
    // session that opened with an exit code would be reporting on a command nobody ran.
    "if [ -n \"$__acter_started\" ]; then printf '\\033]133;D;%s\\033\\\\' \"$__acter_status\"; fi; ",
    "__acter_started=1; ",
    // The user's own prompt, wrapped once and kept. `\[` and `\]` tell readline these
    // bytes take no space on the screen.
    "if [ -z \"$__acter_marked\" ]; then ",
    "PS1='\\[\\033]133;A\\033\\\\\\]'\"$PS1\"'\\[\\033]133;B\\033\\\\\\]'; __acter_marked=1; fi; ",
    // Clear the guard *before* arming the trap, never after: a trap armed inside a
    // function fires for the statements that follow it in that same function.
    "__acter_seen=; trap '__acter_output' DEBUG; }; ",
    "__acter_prompt"
);

/// The environment one WSL session is started with: the injection, and the `WSLENV` entry
/// without which it would not arrive.
///
/// The two are one value because they are one mechanism — a `PROMPT_COMMAND` that does not
/// cross is a session that looks integrated in the launch and marks nothing at all.
pub(crate) const ENVIRONMENT: &[(&str, &str)] = &[WSLENV, ("PROMPT_COMMAND", PROMPT_COMMAND)];

/// The one shell this injection was measured against, and therefore the only one it is
/// given to (spec B5.5, decision 4).
///
/// **The identity may be guessed from a name; the injection may never be.** Every line of
/// the program above cost real measurement — a `DEBUG` trap cannot remove itself from
/// inside a function, and the version built on the wrong assumption emitted four `C`
/// markers per command. Knowing a distribution runs zsh licenses *saying* "zsh" and
/// nothing else until a zsh injection has been measured the same way.
pub(crate) const MEASURED: &str = "bash";

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes matter more than the string does. These are what bash's own prompt
    /// expansion and `printf` turn into `ESC ] 133 ; X ESC \`, and a marker spelled with
    /// `\e` instead of `\033`, or terminated with a bell instead of a string terminator,
    /// is a session that looks integrated in the source and emits nothing recognisable.
    #[test]
    fn every_marker_of_the_full_cycle_is_in_the_program() {
        assert!(
            PROMPT_COMMAND.contains(r"\033]133;A\033\\"),
            "the prompt region opens"
        );
        assert!(
            PROMPT_COMMAND.contains(r"\033]133;B\033\\"),
            "and the command line begins"
        );
        assert!(
            PROMPT_COMMAND.contains(r"\033]133;C\033\\"),
            "output starts where the DEBUG trap fires"
        );
        assert!(
            PROMPT_COMMAND.contains(r"\033]133;D;%s\033\\"),
            "and the command ends with a code rather than without one"
        );
    }

    /// `D` without an exit code is a boundary with no verdict, which is most of what WSL
    /// is here for: it is the first shipped shell that can say how a command went.
    #[test]
    fn the_end_marker_carries_the_status_of_the_command_that_ended() {
        assert!(PROMPT_COMMAND.contains("__acter_status=$?"));
        assert!(PROMPT_COMMAND.contains(r#"'\033]133;D;%s\033\\' "$__acter_status""#));
    }

    /// The status is captured before anything else runs, because defining a function
    /// succeeds and would otherwise report every command as having exited zero.
    #[test]
    fn the_exit_status_is_captured_before_the_first_function_is_defined() {
        let status = PROMPT_COMMAND
            .find("__acter_status=$?")
            .expect("the status is captured");
        let first_definition = PROMPT_COMMAND
            .find("__acter_output()")
            .expect("the trap handler is defined");

        assert!(status < first_definition);
    }

    /// The measured ordering bug, pinned so it cannot come back: arming the `DEBUG` trap
    /// and *then* clearing the guard emits one `C` before the session's first prompt,
    /// because the trap fires on the clearing statement itself.
    #[test]
    fn the_guard_is_cleared_before_the_trap_is_armed_and_never_after() {
        let cleared = PROMPT_COMMAND
            .find("__acter_seen=;")
            .expect("the guard is cleared each prompt");
        let armed = PROMPT_COMMAND
            .find("trap '__acter_output' DEBUG")
            .expect("the trap is armed each prompt");

        assert!(
            cleared < armed,
            "clearing the guard after arming the trap emits a spurious C"
        );
    }

    /// The user's prompt is wrapped, not replaced — and wrapped once, so a session whose
    /// hundredth prompt is drawn does not carry a hundred nested markers.
    #[test]
    fn the_users_own_prompt_is_kept_between_the_markers_and_wrapped_only_once() {
        assert!(
            PROMPT_COMMAND.contains(r#"'\[\033]133;A\033\\\]'"$PS1"'\[\033]133;B\033\\\]'"#),
            "the existing PS1 sits between the two markers"
        );
        assert!(
            PROMPT_COMMAND.contains(r#"if [ -z "$__acter_marked" ]; then"#),
            "and it is only wrapped when it has not been wrapped already"
        );
    }

    /// `\[` and `\]` mark the markers as taking no space. Without them readline
    /// miscounts the prompt's width and a long line wraps in the wrong place, which for
    /// someone reading a line at a time is the difference between a command and two
    /// fragments.
    #[test]
    fn the_markers_in_the_prompt_are_declared_non_printing() {
        assert!(PROMPT_COMMAND.contains(r"\[\033]133;A"));
        assert!(PROMPT_COMMAND.contains(r"\033]133;B\033\\\]"));
    }

    /// Nothing here creates, appends to or sources a file. The pinned constraint of the
    /// whole entry, asserted rather than trusted: an implementation that reached for
    /// `--rcfile` or a heredoc into the user's home would trip this.
    #[test]
    fn the_injection_writes_nothing_into_the_distribution() {
        for forbidden in [">", ">>", "rcfile", "init-file", "source ", "tee ", "mkdir"] {
            assert!(
                !PROMPT_COMMAND.contains(forbidden),
                "the injection never touches the user's filesystem: it contains {forbidden:?}"
            );
        }
    }

    /// Without the `WSLENV` entry the variable simply does not exist inside the
    /// distribution (measured, ROADMAP 22.6), so the session would launch, look
    /// integrated and mark nothing.
    #[test]
    fn the_environment_carries_the_injection_and_the_entry_that_lets_it_cross() {
        assert_eq!(
            ENVIRONMENT,
            &[
                ("WSLENV", "PROMPT_COMMAND"),
                ("PROMPT_COMMAND", PROMPT_COMMAND)
            ]
        );
    }
}
