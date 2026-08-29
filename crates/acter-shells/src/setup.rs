//! Policy: what Acter runs inside a session once that session is established, per shell.
//!
//! **The session is set up after it is established, and nothing is armed at launch** (spec
//! B9.5, decision 1). `wsl/injection.rs` used to put a `PROMPT_COMMAND` into the environment
//! `wsl.exe` was started with, so bash inherited it *before* it sourced `.bashrc` — which
//! meant Acter went first and every startup file the user owns got the last word. A line sent
//! into the session once it is up is the only ordering in which Acter has the last word
//! instead, and it needs no launch arguments, so it works over SSH exactly as it works over
//! WSL (decision 2).
//!
//! **Keyed by the shell's name rather than by the transport**, which is what lets one WSL
//! bash and one SSH bash share a setup for the first time.
//!
//! # Three states, and they are B5.5's three
//!
//! A line and the [`ShellMarkers`] that line earns; a shell that is named with nothing
//! written for it; and a far end that answered nothing, about which nothing is invented.
//! **The identity may be guessed from the name; the setup may never be.** Every line here
//! cost its own measurement, and a shell nobody has done that for is named rather than
//! experimented on (spec B5.5, decision 4, and B9.5, decision 6).
//!
//! # What 23.11 measured, and why this shape defeats it
//!
//! Seven scenarios against real interactive bash on a pseudoconsole inside Ubuntu 24.04 under
//! WSL 2.5.7.0, driven through `script -qec`, with a per-scenario rcfile that proves it was
//! sourced, 2026-08-29. Each run submitted one command that succeeds and one that exits 7, so
//! a wrong exit code is visible rather than inferred.
//!
//! - **A `.bashrc` that assigns `PROMPT_COMMAND` itself** produced no markers at all. That
//!   one was documented and behaved as documented: the grace period fires and the listener
//!   is told.
//! - **A hook appended after ours stole the `C` marker** — `ABCD` became `CABD`, because the
//!   `DEBUG` trap Acter armed as its last statement fired on *their* hook, and the
//!   `__acter_seen` guard then suppressed the marker for the command the user actually typed.
//! - **A hook that rebuilds `PS1` killed `A` and `B` permanently** — `CDCDC`. The old guard
//!   was a one-shot boolean, so once anything rebuilt `PS1` after the first prompt Acter
//!   never wrapped it again. That is the most common prompt pattern there is: starship,
//!   conda, virtualenv and `__git_ps1` all rebuild `PS1` from a `PROMPT_COMMAND` hook.
//! - **A hook prepended before ours announced `D;0` for a command that exited 7.**
//!   `__acter_status=$?` was the first statement of *our string* but not of the *variable*
//!   once somebody prepended, so it captured their hook's status. **Acter said out loud that
//!   a failed command had succeeded**, with markers flowing so confidently that nothing in
//!   the system doubted it.
//!
//! Every one of those exists because Acter went first. Going last and wrapping the user's own
//! hook in a sandwich defeats all four, and was measured to: against the hostile rcfile that
//! assigns `PROMPT_COMMAND` for itself — where the launch-time injection produced **no
//! markers** — the sandwich produced `ABCDABCDABC` with exit codes `0` and `7`.
//!
//! **The premise the whole entry rests on, measured 2026-08-29.** Bash re-reads
//! `PROMPT_COMMAND` before every prompt; it is not a startup-time-only variable. In a session
//! with nothing injected at launch, two commands produced zero markers; the session then
//! assigned `PROMPT_COMMAND` itself, and every prompt after that carried the full cycle —
//! zero markers before the assignment, eleven after, with a command that exited 7 correctly
//! reported as `D;7`. No `export` is needed, and it was measured without one.
//!
//! **The alternative was measured and rejected on reach rather than on merit.** `bash
//! --rcfile <ours>`, where our file sources the user's rc first and then applies the setup,
//! produced the full cycle against the same hostile rcfile and puts nothing in the terminal
//! buffer at all. But it is bash-specific, it changes login-shell startup semantics, and it
//! does not exist over SSH where Acter controls no launch arguments.
//!
//! # The two bash facts this is built on, both measured rather than read
//!
//! These travelled here from `wsl/injection.rs` with their dates intact, because what that
//! module got deleted for is its *mechanism* and not its evidence.
//!
//! **`C` needs a `DEBUG` trap, and the trap cannot disarm itself.** `PROMPT_COMMAND` runs
//! before a prompt, so it can say `D` and `A`, and `PS1` can say `B` — but nothing except the
//! `DEBUG` trap fires between the user pressing Enter and their command running, which is
//! exactly where `C` belongs. The obvious way to fire it once is for the trap to remove
//! itself, and on 2026-08-24 that was measured not to work: a `trap - DEBUG` executed inside
//! a shell function is discarded when the function returns, while a `trap ... DEBUG` set
//! inside one sticks. The version built on self-removal emitted four `C` markers per command
//! instead of one. So the guard is a variable, set by the trap and cleared by the prompt.
//!
//! **A trap armed inside a function fires for the rest of that function.** Arming the trap
//! and *then* clearing the guard produced one spurious `C` before the very first prompt,
//! because the trap fired on the clearing statement itself. Arming last is what removes it.
//!
//! # What is not claimed
//!
//! **Somebody who re-sources `~/.bashrc` afterwards clobbers this**, and nothing can foresee
//! that. It degrades the honest way — markers stop, the grace period expires, the listener is
//! told — which is the failure mode this product already handles well (spec B9.5,
//! decision 1).
//!
//! **A `PROMPT_COMMAND` that is an array is read as its first element.** Bash 5.1 allows one,
//! `"${PROMPT_COMMAND-}"` yields element zero, and the rest of such a hook would stop running.
//! Unmeasured, because no rcfile met in any of 23.11's seven scenarios used one; recorded so
//! that a session where it happens is diagnosed rather than rediscovered.

use acter_core::{SessionSetup, ShellMarkers};

/// The bash program, as one command line.
///
/// Read it as three parts. **Two statements that make this a command like any other** — the
/// `C` it prints for itself and the `started` guard it pre-arms — then the user's existing
/// hook captured, then the prompt handler that will run before every prompt from now on.
///
/// **The first statement is the `C`, and the whole of decision 3 turns on it.** The setup
/// runs before the `DEBUG` trap exists, so no `C` would otherwise arrive; the pump would open
/// the block from the echo instead, the boundary tracker's own `block_open` would stay false,
/// and it would therefore *ignore* the `D` that follows. The block would stay open until the
/// user's first real command — which means `running` true from the moment of connecting, and
/// a Ctrl+C pressed before the first command reported as stopping a command nobody ran.
/// With the `C` printed here and the guard pre-armed, this is a command with the full marker
/// cycle that closes with a real exit code, and neither the pump nor the tracker needs
/// changing.
///
/// **Its exit code is `0` and that is honest.** What the line does is an assignment, and an
/// assignment succeeds. Whether the setup *worked* is a different question, and the grace
/// period is what answers it (spec B9.5, decision 12).
///
/// The sandwich is inside `__acter_prompt` rather than spread across `PROMPT_COMMAND`, which
/// is what makes each of 23.11's four failures unreachable rather than merely unlikely:
/// nothing can be prepended in front of our status capture or appended behind our trap,
/// because there is no seam in the variable to prepend or append to.
pub(crate) const BASH: &str = concat!(
    // Output starts here, for this command, said by the command itself.
    "printf '\\033]133;C\\033\\\\'; ",
    // So the very next prompt emits a `D` rather than skipping it as "no command has run
    // yet". One has: this one.
    "__acter_started=1; ",
    // The user's own hook, captured before it is replaced. `${PROMPT_COMMAND-}` rather than
    // `$PROMPT_COMMAND` so a session running `set -u` is not tripped by a variable that is
    // frequently unset — which, now that nothing is injected at launch, is the ordinary case.
    "__acter_hook=\"${PROMPT_COMMAND-}\"; ",
    // The guard makes this the *first* simple command of whatever the user submitted rather
    // than every one of them: a pipeline or a loop fires the trap repeatedly and only the
    // first firing means "the command began".
    "__acter_output() { if [ -n \"$__acter_seen\" ]; then return; fi; __acter_seen=1; ",
    "printf '\\033]133;C\\033\\\\'; }; ",
    // **The status is captured first, inside our own leading segment.** This is the answer to
    // the failure that made Acter announce a success for a command that exited 7: a function
    // *definition* succeeds, and so does anybody's prompt hook, so anything running before
    // this line would reset `$?` and every command would be reported as having worked.
    "__acter_prompt() { __acter_status=$?; ",
    "if [ -n \"$__acter_started\" ]; then printf '\\033]133;D;%s\\033\\\\' \"$__acter_status\"; fi; ",
    "__acter_started=1; ",
    // **The user's own hook, in the middle.** Before their hooks is where `$?` is still the
    // user's command's status; after them is where a rebuilt `PS1` can be re-wrapped and
    // where the trap can be armed with nothing left to steal its first firing. Going first or
    // last alone cannot do both.
    "eval \"$__acter_hook\"; ",
    // **The guard is a test of `PS1`'s own contents, not a one-shot boolean.** That is the
    // fix for `CDCDC`: a prompt rebuilt at any later moment — by starship, conda, virtualenv
    // or `__git_ps1` — is simply wrapped again on the next cycle.
    "case \"$PS1\" in *'133;A'*) ;; ",
    // The user's prompt is wrapped and kept, never rebuilt. `\\[` and `\\]` tell readline
    // these bytes take no space, so a long line still wraps where it always did.
    "*) PS1='\\[\\033]133;A\\033\\\\\\]'\"$PS1\"'\\[\\033]133;B\\033\\\\\\]';; esac; ",
    // Clear the guard *before* arming the trap, never after: a trap armed inside a function
    // fires for the statements that follow it in that same function.
    "__acter_seen=; trap '__acter_output' DEBUG; }; ",
    "PROMPT_COMMAND=__acter_prompt"
);

/// The POSIX `sh` program, as one command line.
///
/// **Its own program and not bash's with a substitution** (spec B9.5, decision 8). `\[` and
/// `\]` are readline's, and dash has no readline: it would print them into the prompt as
/// literal characters. Every line of a setup costs its own measurement, which is the rule
/// that produced this program rather than an exception to it.
///
/// **It reaches the prompt boundaries and no further**, which is why [`setup_for`] answers
/// [`ShellMarkers::PromptAndCommandLine`] for it: POSIX `sh` has `PS1` but no prompt hook and
/// no `DEBUG` trap, so there is nowhere for "output starts here" or "the command ended with
/// this code" to come from. That is not a reason to refuse it — `cmd.exe` already ships the
/// same claim and a listener gets real value from prompt boundaries alone — and the tracker
/// synthesizes the missing `C` at the end of the echo exactly as it does for cmd.
///
/// **The markers are built with `printf` into the value rather than written as escapes.**
/// `sh` expands parameters in `PS1`, so `\033` written into the variable would be four
/// characters drawn on the screen. A command substitution puts the real bytes there once, at
/// setup time.
///
/// **It ends its markers with a bell rather than with a string terminator, and that is a
/// measurement rather than a preference.** Measured 2026-08-29 against `docker-desktop`
/// (busybox 1.37.0 ash, `/bin/sh` → `/bin/busybox`), driven through `script -qc` on a
/// pseudoconsole: with `ESC \` as the terminator, `A` came out as
/// `ESC ] 133 ; A ESC` *carriage return* and the prompt `rc$ ` was drawn as `c$ ` — busybox
/// expands backslash escapes in `PS1`, so the terminator's trailing `\` glued itself to the
/// `r` of the prompt and became a `\r`. **The first character of the user's own prompt
/// disappeared.** A bell terminator is the other spelling OSC allows, it is what this crate's
/// own sniffer already reads, and it puts no backslash in `PS1` at all.
///
/// This is decision 8's rule meeting a second instance of itself: `\[` and `\]` are
/// readline's, and what a shell does to a backslash is its own business. Every line of a
/// setup costs its own measurement.
///
/// **And the cost this line still carries, measured rather than suspected: sixteen columns.**
/// `\[` and `\]` are how a shell is told that what follows occupies no width, and busybox has
/// no equivalent — so the bytes that make the prompt *draw* correctly are still counted by its
/// line editor. Measured 2026-08-29 on a pseudoconsole fixed at 80 columns, typing into a
/// prompt four visible columns wide: unmarked, busybox begins a second row at 76 characters
/// (4 + 76 = 80); wrapped in these two markers, at 60 (4 + 16 + 60). Sixteen bytes, sixteen
/// columns, none of them skipped. bash under the same measurement moves the cursor right by
/// exactly the four columns its prompt occupies, which is `\[` and `\]` doing their job.
///
/// **And busybox does have an equivalent, which is why the line below is not the simple one.**
/// Measured 2026-08-29, the same boundary at 60, 68 and 76 characters: a prompt wrapped in
/// `\[` and `\]` puts the boundary back at **76, exactly where the unmarked control is**, and
/// draws byte-identically — the brackets are consumed by busybox's own prompt parser and never
/// reach the screen. So the sixteen columns are not a property of the markers; they are the
/// cost of not telling this shell about them.
///
/// **But `\[` may not simply be added, because dash is not busybox.** The same measurement
/// against dash 0.5.12 on Ubuntu draws them literally: `\[ESC]133;A BEL\]rc$ `, brackets and
/// all, in the user's own prompt. dash has no line editor to miscount anything, so it needs no
/// brackets and must not be given them; busybox needs them and honours them; and both answer to
/// the name `sh`, which is why the name cannot decide it.
///
/// **`BB_ASH_VERSION` decides it, at runtime, inside the line itself.** Measured the same day:
/// busybox ash sets it (`1.37.0` on `docker-desktop`), dash leaves it empty. So the setup asks
/// the shell which one it is and wraps accordingly — one line, one round trip, and no guess
/// from a name that two different shells share.
///
/// It prints its own `C` for [`BASH`]'s reason and for a slightly different consequence: with
/// no `D` in this shell the block opened by the echo is closed by the next prompt's `B`, and
/// the tracker only closes a block it knows is open.
pub(crate) const SH: &str = concat!(
    "printf '\\033]133;C\\007'; ",
    "if [ -n \"$BB_ASH_VERSION\" ]; then ",
    "PS1=\"\\[$(printf '\\033]133;A\\007')\\]$PS1\\[$(printf '\\033]133;B\\007')\\]\"; ",
    "else ",
    "PS1=\"$(printf '\\033]133;A\\007')$PS1$(printf '\\033]133;B\\007')\"; ",
    "fi"
);

/// What Acter runs inside a session at this far end, by the name the far end gave.
///
/// `None` is two situations and they are alike from here: a far end that answered nothing, and
/// one running a shell nobody has measured a setup for. Both start, both are named as far as
/// they can be, and neither is experimented on — the connection sentence says which of the two
/// it was (spec B9.5, decision 13).
pub fn setup_for(shell: Option<&str>) -> Option<SessionSetup> {
    let (line, markers) = match shell? {
        "bash" => (BASH, ShellMarkers::Full),
        "sh" | "dash" => (SH, ShellMarkers::PromptAndCommandLine),
        // Named and nothing more. `zsh` and `fish` both have prompt hooks — `precmd` and
        // `fish_prompt` — and each becomes one small additive entry, measured the way bash's
        // was. Guessing at one from the shape of bash's is exactly what B5.5 forbids.
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

    /// The bytes matter more than the string does. These are what bash's own prompt expansion
    /// and `printf` turn into `ESC ] 133 ; X ESC \`, and a marker spelled with `\e` instead of
    /// `\033`, or terminated with a bell instead of a string terminator, is a session that
    /// looks integrated in the source and emits nothing recognisable.
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

    /// **The whole of decision 3, asserted where somebody shortening this line will meet it.**
    /// Without the `C` first, no `C` ever arrives for the setup command — the trap does not
    /// exist yet — so the tracker never opens the block, ignores the `D` that follows, and
    /// leaves the session reporting "running" from the moment it connects.
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
    }

    /// And the other half of decision 3: the guard is pre-armed, so the next prompt closes
    /// this command with a real `D` rather than skipping it as "nothing has run yet".
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

    /// `D` without an exit code is a boundary with no verdict, which is most of what bash is
    /// here for: it is the shell that can say how a command went.
    #[test]
    fn the_end_marker_carries_the_status_of_the_command_that_ended() {
        assert!(BASH.contains("__acter_status=$?"));
        assert!(BASH.contains(r#"'\033]133;D;%s\033\\' "$__acter_status""#));
    }

    /// **The failure that made Acter announce a success for a command that exited 7**, pinned.
    /// The status is captured as the first statement of the prompt handler, before the user's
    /// own hook and before anything that could succeed on its own behalf.
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

    /// **The sandwich, asserted as an order** (spec B9.5, decision 7). The user's hook runs
    /// after our status capture and before our re-wrap and our trap: before their hooks is
    /// where `$?` is still theirs, and after them is where a rebuilt `PS1` can be re-wrapped
    /// and the trap armed with nothing left to steal its first firing.
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

    /// The measured ordering bug, pinned so it cannot come back: arming the `DEBUG` trap and
    /// *then* clearing the guard emits one `C` before the session's first prompt, because the
    /// trap fires on the clearing statement itself.
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

    /// **The `CDCDC` failure, pinned.** The old guard was `__acter_marked`, a one-shot
    /// boolean, so the first prompt rebuild after the first prompt lost `A` and `B` forever.
    /// Testing `PS1` itself is what makes a rebuild at any later moment cost one cycle rather
    /// than the session.
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

    /// The user's prompt is wrapped, not replaced, and the markers inside it are declared
    /// non-printing — without which readline miscounts the prompt's width and a long line
    /// wraps in the wrong place, which for somebody reading a line at a time is the
    /// difference between a command and two fragments.
    #[test]
    fn the_users_own_prompt_is_kept_between_the_markers_and_declared_non_printing() {
        assert!(
            BASH.contains(r#"'\[\033]133;A\033\\\]'"$PS1"'\[\033]133;B\033\\\]'"#),
            "the existing PS1 sits between the two markers"
        );
    }

    /// **`sh` is its own program and not bash's with a substitution** (spec B9.5,
    /// decision 8), and it is now two programs in one line because two shells answer to that
    /// name and they disagree about `\[`.
    ///
    /// **The branch dash takes carries no brackets**, which is the assertion this test was
    /// written for and still is: measured 2026-08-29, dash draws them into the prompt as
    /// literal characters, in front of somebody who cannot see that they are there. **The
    /// branch busybox takes carries them**, because measured the same day busybox honours them
    /// and without them its line editor counts sixteen bytes it cannot see (roadmap 23.14).
    #[test]
    fn only_the_branch_for_a_shell_that_honours_them_carries_the_non_printing_brackets() {
        let (busybox, dash) = SH.split_once("else ").expect("the line branches");

        assert!(
            busybox.contains(r"\[$(printf '\033]133;A\007')\]"),
            "busybox is told the marker takes no columns: {busybox}"
        );
        assert!(
            !dash.contains(r"\["),
            "and dash, which would print it literally, is not: {dash}"
        );
        assert!(!dash.contains(r"\]"), "nor this: {dash}");
    }

    /// **The shell is asked which it is rather than guessed at from its name**, because
    /// `/bin/sh` is busybox on one distribution and dash on the next and the probe answers
    /// `sh` for both. Measured 2026-08-29: busybox ash sets `BB_ASH_VERSION` (`1.37.0` on
    /// `docker-desktop`), dash leaves it empty.
    #[test]
    fn the_sh_program_asks_the_shell_which_shell_it_is() {
        assert!(
            SH.contains(r#"if [ -n "$BB_ASH_VERSION" ]; then "#),
            "one round trip, no guess from a shared name: {SH}"
        );
        assert!(SH.ends_with("fi"), "and the branch is closed: {SH}");
    }

    /// `sh` marks where the prompt begins and where the command line does, and nothing else —
    /// so what it can do is asserted here rather than inferred from the absence of a `D`.
    #[test]
    fn the_sh_program_marks_the_prompt_and_nothing_after_it() {
        assert!(SH.contains(r"\033]133;A\007"), "the prompt region opens");
        assert!(SH.contains(r"\033]133;B\007"), "the command line begins");
        assert!(
            !SH.contains("133;D"),
            "and no verdict is forged for a shell that has nowhere to compute one"
        );
    }

    /// **The character the user's prompt lost, pinned** (measured 2026-08-29 against
    /// `docker-desktop`). busybox expands backslash escapes in `PS1`, so a marker terminated
    /// with `ESC \` glues its trailing backslash to whatever the prompt begins with: `rc$ `
    /// was drawn as `c$ `, in front of somebody who cannot see that a character is missing.
    /// A bell terminator puts no backslash in `PS1` at all.
    #[test]
    fn the_sh_program_puts_no_backslash_into_the_prompt() {
        let prompt = SH.split_once("PS1=").expect("the prompt is wrapped").1;

        assert!(
            !prompt.contains(r"\\"),
            "a backslash here eats the first character of the user's own prompt: {prompt}"
        );
    }

    /// Nothing here creates, appends to or sources a file. **The pinned constraint of the
    /// whole approach** (spec B5.3, decision 2, kept by B9.5, decision 1): nothing is written
    /// into the distribution or onto the host, so nothing is left behind on a machine the
    /// user has to live in after Acter closes.
    #[test]
    fn no_setup_writes_anything_into_the_far_end() {
        for line in [BASH, SH] {
            for forbidden in [">", ">>", "rcfile", "init-file", "source ", "tee ", "mkdir"] {
                assert!(
                    !line.contains(forbidden),
                    "a setup never touches the user's filesystem: {line} contains {forbidden:?}"
                );
            }
        }
    }

    /// The two shells with a measured setup, and what each earns.
    #[test]
    fn the_measured_shells_answer_with_the_markers_their_line_earns() {
        let bash = setup_for(Some("bash")).expect("bash has a measured setup");
        assert_eq!(bash.line, BASH);
        assert_eq!(bash.markers, ShellMarkers::Full);

        let sh = setup_for(Some("sh")).expect("sh has a measured setup");
        assert_eq!(sh.line, SH);
        assert_eq!(
            sh.markers,
            ShellMarkers::PromptAndCommandLine,
            "the sentence has to be able to say partly"
        );
    }

    /// **The rule that keeps this honest.** A shell Acter can name is named, and nothing is
    /// run in it until somebody has measured what to run — which is B5.5's rule surviving the
    /// change of mechanism intact.
    #[test]
    fn a_shell_nobody_measured_is_named_without_being_set_up() {
        for named in ["zsh", "fish", "nu", "ksh", "Bash", "bash5", "csh"] {
            assert_eq!(
                setup_for(Some(named)),
                None,
                "{named} has no measured setup, so nothing is run in it"
            );
        }
    }

    /// A far end that said nothing is the honest absence, not a default.
    #[test]
    fn a_far_end_that_answered_nothing_has_nothing_run_in_it() {
        assert_eq!(setup_for(None), None);
    }

    /// Every setup is one line, because it is submitted as one command. A newline in here
    /// would be two commands, the second of which nobody authorised and nothing heads.
    #[test]
    fn every_setup_is_a_single_command_line() {
        for line in [BASH, SH] {
            assert!(!line.contains('\n'), "a setup is one submission: {line}");
            assert!(!line.contains('\r'), "a setup is one submission: {line}");
        }
    }
}
