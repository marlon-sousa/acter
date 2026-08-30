//! Entity/value: which OSC 133 markers the shell at the far end is able to emit.
//!
//! A shell's own limitation, declared rather than discovered. `cmd.exe`'s `PROMPT`
//! understands `$e` as escape, so the prompt itself can carry `A` and `B`; what it has no
//! equivalent of is a post-execution hook, so "output starts here" and "the command ended
//! with this code" have nowhere to come from without a third-party layer such as Clink,
//! which is not a dependency this product takes (spec B4.5, decision 1).
//!
//! **Not an [`Integration`](crate::Integration) state**, and that was decided before any
//! code: a shell that marks `A` and `B` is `Integrated`, because its prompt region and its
//! command line are genuinely delimited. What changes is what the boundary tracker must
//! supply for itself and what counts as a block's content, and both of those are questions
//! about the *shell*, not about whether markers arrived.
//!
//! It is a value and not a port. Which shell to run and what to inject into it is
//! `ShellAdapter`'s knowledge and therefore B5's; carrying the answer as a value lets B4.5
//! use it without deciding that port's shape a release early.

/// What the far end's prompt is able to say about command boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellMarkers {
    /// `A`, `B`, `C` and `D` — the full cycle, and every session's assumption until B4.5.
    /// A shell that emits nothing at all is also this: it simply never becomes integrated.
    #[default]
    Full,
    /// `A` and `B` only: the prompt region and the start of the command line are marked,
    /// and nothing marks where output begins or how the command ended.
    ///
    /// Two consequences, both in the tracker and the service rather than here.
    /// `C` is synthesized at the end of the echoed line — in a line-oriented shell the
    /// echo is the one line the shell read, so that is evidence rather than a guess. And a
    /// verdict never becomes available, so the returning prompt is the only ending such a
    /// session has to offer a listener.
    PromptAndCommandLine,
    /// `A`, `B` and `D` — the prompt region, the start of the command line, and how the
    /// command ended, with nothing marking where output begins.
    ///
    /// **POSIX `sh`, and it took a wrong belief to find it** (roadmap 23.15). B9.5 decision 8
    /// gave `sh` [`Self::PromptAndCommandLine`] on the reasoning that a verdict needs a
    /// post-execution hook and POSIX `sh` has none. The reasoning is sound and the conclusion
    /// was wrong: `PS1` is expanded *every time the prompt is drawn*, and `$?` at that moment
    /// is the status of the command that just finished. Measured 2026-08-29 against busybox
    /// 1.37.0 and dash 0.5.12, `PS1='[status=$?]# '` reports `0` after `true` and `7` after
    /// `(exit 7)` in both.
    ///
    /// So the exit marker goes at the front of the prompt string and the shell fills the
    /// number in itself. What such a shell still cannot say is where output begins — there is
    /// no hook between Enter and the command running — so the tracker synthesizes `C` at the
    /// end of the echoed line exactly as it does for [`Self::PromptAndCommandLine`].
    PromptCommandLineAndExitCode,
}

impl ShellMarkers {
    /// Whether the far end marks where a command's output begins.
    ///
    /// The question every rule that changes actually asks, so it is asked once here rather
    /// than matched on in three places.
    pub fn marks_output_start(self) -> bool {
        matches!(self, Self::Full)
    }

    /// Whether the far end says how a command ended.
    ///
    /// **The other half of the question, and until roadmap 23.15 nothing could tell them
    /// apart.** Two rules in the session asked "is this `Full`" when what they meant was this:
    /// whether the prompt is its own announcement rather than a block's only ending, and
    /// whether the prompt belongs in a block's content. Both follow from having a verdict and
    /// neither follows from marking output start, which is why they are asked separately now
    /// that a shell exists that answers them differently.
    pub fn reports_exit_code(self) -> bool {
        matches!(self, Self::Full | Self::PromptCommandLineAndExitCode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_shell_marks_where_output_begins() {
        assert!(ShellMarkers::Full.marks_output_start());
    }

    #[test]
    fn a_prompt_only_shell_does_not() {
        assert!(!ShellMarkers::PromptAndCommandLine.marks_output_start());
    }

    /// A `sh` that reports its exit codes still cannot say where output begins, which is what
    /// keeps the tracker synthesizing a `C` for it.
    #[test]
    fn a_shell_that_reports_exit_codes_need_not_mark_where_output_begins() {
        assert!(!ShellMarkers::PromptCommandLineAndExitCode.marks_output_start());
        assert!(ShellMarkers::PromptCommandLineAndExitCode.reports_exit_code());
    }

    /// **The question two rules in the session were asking through the wrong half** (roadmap
    /// 23.15): the prompt is content, rather than its own announcement, exactly when no
    /// verdict exists to end a command with.
    #[test]
    fn only_the_prompt_only_shell_has_no_verdict_to_offer() {
        assert!(ShellMarkers::Full.reports_exit_code());
        assert!(!ShellMarkers::PromptAndCommandLine.reports_exit_code());
    }

    /// The default is what every session assumed before this type existed, so adding it
    /// changes nothing that did not opt in.
    #[test]
    fn the_default_is_the_full_cycle() {
        assert_eq!(ShellMarkers::default(), ShellMarkers::Full);
    }
}
