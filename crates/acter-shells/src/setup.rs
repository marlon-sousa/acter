//! Policy: what Acter runs inside a session once that session is established, per shell.

use acter_core::{SessionSetup, ShellMarkers};

/// The leading `C` and the pre-armed `__acter_started` must stay: without them the tracker never
/// opens the setup's own block, ignores the `D` that follows, and reports a command running from
/// the moment the session connects.
///
/// bash on Ubuntu 24.04 under WSL 2.5.7.0 re-reads `PROMPT_COMMAND` before every prompt, so this
/// assignment takes effect in a running session without `export`.
///
/// A `~/.bashrc` re-sourced after this line replaces it, and an array `PROMPT_COMMAND` is
/// captured as its first element only.
pub(crate) const BASH: &str = concat!(
    "printf '\\033]133;C\\033\\\\'; ",
    "__acter_started=1; ",
    // `-` so a session under `set -u` does not fail on an unset `PROMPT_COMMAND`.
    "__acter_hook=\"${PROMPT_COMMAND-}\"; ",
    // The `DEBUG` trap fires per simple command and `trap - DEBUG` inside a function is undone
    // when it returns, so a variable guard keeps `C` to the first firing.
    "__acter_output() { if [ -n \"$__acter_seen\" ]; then return; fi; __acter_seen=1; ",
    "printf '\\033]133;C\\033\\\\'; }; ",
    // `$?` is read before anything else in the handler, or it is another statement's status.
    "__acter_prompt() { __acter_status=$?; ",
    "if [ -n \"$__acter_started\" ]; then printf '\\033]133;D;%s\\033\\\\' \"$__acter_status\"; fi; ",
    "__acter_started=1; ",
    "eval \"$__acter_hook\"; ",
    "case \"$PS1\" in *'133;A'*) ;; ",
    "*) PS1='\\[\\033]133;A\\033\\\\\\]'\"$PS1\"'\\[\\033]133;B\\033\\\\\\]';; esac; ",
    // Clear the guard before arming the trap: a trap armed in a function fires for the rest of it.
    "__acter_seen=; trap '__acter_output' DEBUG; }; ",
    "PROMPT_COMMAND=__acter_prompt"
);

/// busybox 1.37.0 and dash 0.5.12 expand `$?` in `PS1` to the status of the last command, so
/// `\$?` is left unexpanded here for the shell to fill in at every prompt.
///
/// busybox 1.37.0 expands backslash escapes in `PS1`, so an `ESC \` terminator eats the first
/// character of the user's prompt; these markers end with BEL.
///
/// busybox 1.37.0 honours `\[` and `\]` and otherwise counts the marker bytes as prompt
/// columns; dash 0.5.12 draws them literally.
///
/// busybox 1.37.0 ash sets `BB_ASH_VERSION` and dash 0.5.12 does not; macOS `/bin/sh` is bash
/// 3.2.57 in POSIX mode, sets `BASH_VERSION` and honours `\[` and `\]`.
pub(crate) const SH: &str = concat!(
    "printf '\\033]133;C\\007'; ",
    "if [ -n \"$BB_ASH_VERSION\" ] || [ -n \"$BASH_VERSION\" ]; then ",
    "PS1=\"\\[$(printf '\\033]133;D;')\\$?$(printf '\\007\\033]133;A\\007')\\]$PS1\\[$(printf '\\033]133;B\\007')\\]\"; ",
    "else ",
    "PS1=\"$(printf '\\033]133;D;')\\$?$(printf '\\007\\033]133;A\\007')$PS1$(printf '\\033]133;B\\007')\"; ",
    "fi"
);

/// zsh 5.9 expands `%?` from a status taken before `precmd` hooks run, fires `preexec` once per
/// submitted line (pipelines, loops and `;` lists included), and honours `%{` and `%}`, so the
/// markers cost no columns.
///
/// A `precmd` hook added after this line runs after ours, so a prompt it rebuilds is not
/// re-wrapped.
pub(crate) const ZSH: &str = concat!(
    r"print -n $'\e]133;C\a'; ",
    r"__acter_pre=$'%{\e]133;D;%?\a\e]133;A\a%}'; ",
    r"__acter_post=$'%{\e]133;B\a%}'; ",
    r"__acter_output() { print -n $'\e]133;C\a' }; ",
    r#"__acter_prompt() { [[ $PROMPT == *'133;A'* ]] || PROMPT="$__acter_pre$PROMPT$__acter_post" }; "#,
    "typeset -ga precmd_functions preexec_functions; ",
    "precmd_functions+=(__acter_prompt); ",
    "preexec_functions+=(__acter_output)"
);

/// None: the far end answered nothing, or runs a shell with no measured setup.
pub fn setup_for(shell: Option<&str>) -> Option<SessionSetup> {
    let (line, markers) = match shell? {
        "bash" => (BASH, ShellMarkers::Full),
        "zsh" => (ZSH, ShellMarkers::Full),
        "sh" | "dash" => (SH, ShellMarkers::PromptCommandLineAndExitCode),
        _ => return None,
    };
    Some(SessionSetup {
        line: line.to_owned(),
        markers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_marker_of_the_full_cycle_is_in_the_bash_program() {
        assert!(
            BASH.contains(r"\033]133;A\033\\"),
            "the prompt region opens"
        );
        assert!(
            BASH.contains(r"\033]133;B\033\\"),
            "the command line begins"
        );
        assert!(
            BASH.contains(r"\033]133;C\033\\"),
            "output starts where the DEBUG trap fires"
        );
        assert!(
            BASH.contains(r"\033]133;D;%s\033\\"),
            "and the command ends with a code rather than without one"
        );
    }

    #[test]
    fn the_setup_marks_its_own_output_before_it_does_anything_else() {
        assert!(
            BASH.starts_with(r"printf '\033]133;C\033\\'; "),
            "the C is the first statement of the line: {BASH}"
        );
        assert!(
            SH.starts_with(r"printf '\033]133;C\007'; "),
            "in every shell, for the same reason: {SH}"
        );
        assert!(
            ZSH.starts_with(r"print -n $'\e]133;C\a'; "),
            "in zsh's own spelling, and the same first statement: {ZSH}"
        );
    }

    #[test]
    fn the_setup_arms_the_guard_that_lets_the_next_prompt_close_it() {
        let armed = BASH
            .find("__acter_started=1")
            .expect("the guard is pre-armed");
        let prompt = BASH
            .find("__acter_prompt()")
            .expect("the prompt handler is defined");

        assert!(
            armed < prompt,
            "it is armed by the line itself, not only by the handler it installs"
        );
    }

    #[test]
    fn the_end_marker_carries_the_status_of_the_command_that_ended() {
        assert!(BASH.contains("__acter_status=$?"));
        assert!(BASH.contains(r#"'\033]133;D;%s\033\\' "$__acter_status""#));
    }

    #[test]
    fn the_exit_status_is_captured_before_anything_else_in_the_prompt_handler() {
        let handler = BASH
            .find("__acter_prompt() {")
            .expect("the prompt handler is defined");
        let status = BASH[handler..]
            .find("__acter_status=$?")
            .expect("the status is captured");
        let hook = BASH[handler..]
            .find("eval \"$__acter_hook\"")
            .expect("the user's own hook runs");

        assert!(
            status < hook,
            "a hook running first would hand us its own status, not the user's command's"
        );
    }

    #[test]
    fn the_users_own_hook_runs_in_the_middle_of_ours() {
        let hook = BASH
            .find("eval \"$__acter_hook\"")
            .expect("their hook runs");
        let wrap = BASH
            .find("case \"$PS1\" in")
            .expect("ours re-wraps the prompt");
        let trap = BASH
            .find("trap '__acter_output' DEBUG")
            .expect("and arms the trap");

        assert!(
            hook < wrap,
            "a rebuilt PS1 is re-wrapped after they rebuild it"
        );
        assert!(
            hook < trap,
            "and nothing of theirs runs after the trap is armed"
        );
    }

    #[test]
    fn the_guard_is_cleared_before_the_trap_is_armed_and_never_after() {
        let cleared = BASH
            .find("__acter_seen=;")
            .expect("the guard is cleared each prompt");
        let armed = BASH
            .find("trap '__acter_output' DEBUG")
            .expect("the trap is armed each prompt");

        assert!(
            cleared < armed,
            "clearing the guard after arming the trap emits a spurious C"
        );
    }

    #[test]
    fn the_prompt_guard_tests_the_prompt_rather_than_remembering_a_boolean() {
        assert!(
            BASH.contains("case \"$PS1\" in *'133;A'*"),
            "the guard asks whether this prompt carries the marker: {BASH}"
        );
        assert!(
            !BASH.contains("__acter_marked"),
            "the one-shot boolean is gone rather than kept beside its replacement"
        );
    }

    #[test]
    fn the_users_own_prompt_is_kept_between_the_markers_and_declared_non_printing() {
        assert!(
            BASH.contains(r#"'\[\033]133;A\033\\\]'"$PS1"'\[\033]133;B\033\\\]'"#),
            "the existing PS1 sits between the two markers"
        );
    }

    #[test]
    fn only_the_branch_for_a_shell_that_honours_them_carries_the_non_printing_brackets() {
        let (busybox, dash) = SH.split_once("else ").expect("the line branches");

        assert!(
            busybox.contains(r"\033]133;A\007')\]"),
            "busybox is told the markers take no columns: {busybox}"
        );
        assert!(
            !dash.contains(r"\["),
            "and dash, which would print it literally, is not: {dash}"
        );
        assert!(!dash.contains(r"\]"), "nor this: {dash}");
    }

    #[test]
    fn the_sh_program_asks_the_shell_which_shell_it_is() {
        assert!(
            SH.contains(r#"if [ -n "$BB_ASH_VERSION" ] || [ -n "$BASH_VERSION" ]; then "#),
            "one round trip, no guess from a name three shells share: {SH}"
        );
        assert!(SH.ends_with("fi"), "and the branch is closed: {SH}");
    }

    #[test]
    fn the_branch_asks_what_the_shell_is_rather_than_what_it_is_called() {
        assert!(
            !SH.contains("$0"),
            "the name a shell was started under decides nothing here: {SH}"
        );
        for variable in ["BB_ASH_VERSION", "BASH_VERSION"] {
            assert!(
                SH.contains(&format!("${variable}")),
                "{variable} is what a shell that needs the brackets sets: {SH}"
            );
        }
    }

    #[test]
    fn the_sh_program_marks_the_prompt_and_says_how_the_command_went() {
        assert!(SH.contains(r"\033]133;A\007"), "the prompt region opens");
        assert!(SH.contains(r"\033]133;B\007"), "the command line begins");
        assert!(
            SH.contains(r"\033]133;D;"),
            "and the last command's verdict arrives with the next prompt: {SH}"
        );
    }

    #[test]
    fn the_exit_code_is_left_for_the_shell_to_fill_in_at_every_prompt() {
        assert_eq!(
            SH.matches(r"\$?").count(),
            2,
            "once on each branch, escaped so the assignment does not expand it: {SH}"
        );
        assert!(
            !SH.contains(r"133;D;$?"),
            "an unescaped $? is this session's first exit code, forever: {SH}"
        );
    }

    #[test]
    fn the_exit_code_is_inside_the_brackets_busybox_honours() {
        let (busybox, _) = SH.split_once("else ").expect("the line branches");
        let opened = busybox.find(r"\[").expect("the busybox branch opens one");
        let verdict = busybox.find("133;D;").expect("and carries the verdict");
        let closed = busybox.find(r"\]").expect("and closes it");

        assert!(
            opened < verdict && verdict < closed,
            "the exit code takes no columns and must be declared so: {busybox}"
        );
    }

    #[test]
    fn the_sh_program_puts_no_backslash_into_the_prompt() {
        let prompt = SH.split_once("PS1=").expect("the prompt is wrapped").1;

        assert!(
            !prompt.contains(r"\\"),
            "a backslash here eats the first character of the user's own prompt: {prompt}"
        );
    }

    #[test]
    fn no_setup_writes_anything_into_the_far_end() {
        for line in [BASH, ZSH, SH] {
            for forbidden in [">", ">>", "rcfile", "init-file", "source ", "tee ", "mkdir"] {
                assert!(
                    !line.contains(forbidden),
                    "a setup never touches the user's filesystem: {line} contains {forbidden:?}"
                );
            }
        }
    }

    #[test]
    fn the_measured_shells_answer_with_the_markers_their_line_earns() {
        let bash = setup_for(Some("bash")).expect("bash has a measured setup");
        assert_eq!(bash.line, BASH);
        assert_eq!(bash.markers, ShellMarkers::Full);

        let zsh = setup_for(Some("zsh")).expect("zsh has a measured setup");
        assert_eq!(zsh.line, ZSH);
        assert_eq!(
            zsh.markers,
            ShellMarkers::Full,
            "zsh reaches the whole cycle bash does, by a shorter route (B5.8)"
        );

        let sh = setup_for(Some("sh")).expect("sh has a measured setup");
        assert_eq!(sh.line, SH);
        assert_eq!(
            sh.markers,
            ShellMarkers::PromptCommandLineAndExitCode,
            "a shell that can say how a command went gets the sentence bash gets (23.15)"
        );

        let dash = setup_for(Some("dash")).expect("dash runs the same measured line");
        assert_eq!(dash.markers, sh.markers);
    }

    #[test]
    fn a_shell_nobody_measured_is_named_without_being_set_up() {
        for named in ["fish", "nu", "ksh", "Bash", "bash5", "csh"] {
            assert_eq!(
                setup_for(Some(named)),
                None,
                "{named} has no measured setup, so nothing is run in it"
            );
        }
    }

    #[test]
    fn a_far_end_that_answered_nothing_has_nothing_run_in_it() {
        assert_eq!(setup_for(None), None);
    }

    #[test]
    fn every_setup_is_a_single_command_line() {
        for line in [BASH, ZSH, SH] {
            assert!(!line.contains('\n'), "a setup is one submission: {line}");
            assert!(!line.contains('\r'), "a setup is one submission: {line}");
        }
    }

    #[test]
    fn every_marker_of_the_full_cycle_is_in_the_zsh_program() {
        assert!(ZSH.contains(r"\e]133;A\a"), "the prompt region opens");
        assert!(ZSH.contains(r"\e]133;B\a"), "the command line begins");
        assert!(
            ZSH.contains(r"\e]133;C\a"),
            "output starts where preexec fires"
        );
        assert!(
            ZSH.contains(r"\e]133;D;%?\a"),
            "and the command ends with a code rather than without one: {ZSH}"
        );
    }

    #[test]
    fn the_zsh_verdict_is_a_prompt_escape_rather_than_a_captured_variable() {
        assert!(
            ZSH.contains("133;D;%?"),
            "the shell fills the number in at every prompt: {ZSH}"
        );
        assert!(
            !ZSH.contains("__acter_status"),
            "nothing captures a status here, so nothing can capture the wrong one: {ZSH}"
        );
    }

    #[test]
    fn the_zsh_program_carries_no_guard_bash_needed_and_zsh_does_not() {
        assert!(
            !ZSH.contains("__acter_seen"),
            "preexec fires once a line, so nothing is deduplicated: {ZSH}"
        );
        assert!(
            !ZSH.contains("__acter_started"),
            "the D lives in the prompt, so the first prompt closes the setup itself: {ZSH}"
        );
    }

    #[test]
    fn the_zsh_program_appends_to_the_hook_arrays_and_never_assigns_them() {
        assert!(ZSH.contains("precmd_functions+=(__acter_prompt)"));
        assert!(ZSH.contains("preexec_functions+=(__acter_output)"));
        assert!(
            !ZSH.contains("precmd_functions=("),
            "an assignment drops the user's own hooks: {ZSH}"
        );
        assert!(
            !ZSH.contains("preexec_functions=("),
            "an assignment drops the user's own hooks: {ZSH}"
        );
        assert!(
            ZSH.contains("typeset -ga precmd_functions preexec_functions"),
            "and a session with neither array yet gets them: {ZSH}"
        );
    }

    #[test]
    fn the_zsh_prompt_guard_tests_the_prompt_rather_than_remembering_a_boolean() {
        assert!(
            ZSH.contains("[[ $PROMPT == *'133;A'* ]]"),
            "the guard asks whether this prompt carries the marker: {ZSH}"
        );
    }

    #[test]
    fn the_users_own_zsh_prompt_is_kept_between_markers_declared_non_printing() {
        assert!(
            ZSH.contains(r#"PROMPT="$__acter_pre$PROMPT$__acter_post""#),
            "the existing PROMPT sits between the two halves: {ZSH}"
        );
        for half in [r"$'%{\e]133;D;%?\a\e]133;A\a%}'", r"$'%{\e]133;B\a%}'"] {
            assert!(
                ZSH.contains(half),
                "each half opens and closes zsh's non-printing brackets: {half}"
            );
        }
    }
}
