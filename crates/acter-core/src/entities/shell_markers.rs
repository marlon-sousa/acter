//! Entity/value: which OSC 133 markers the shell at the far end is able to emit.
//!
//! A shell's own limitation, declared rather than discovered. `cmd.exe`'s `PROMPT`
//! understands `$e` as escape, so the prompt itself can carry `A` and `B`; it has no
//! post-execution hook, so `C` and `D` have nowhere to come from without a third-party
//! layer such as Clink.
//!
//! Not an [`Integration`](crate::Integration) state: a shell that marks `A` and `B` is
//! `Integrated`, because its prompt region and its command line are genuinely delimited.
//! What changes is what the boundary tracker must supply for itself and what counts as a
//! block's content.

/// What the far end's prompt is able to say about command boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellMarkers {
    /// `A`, `B`, `C` and `D`. A shell that emits nothing at all is also this: it simply
    /// never becomes integrated.
    #[default]
    Full,
    /// `A` and `B` only: the prompt region and the start of the command line are marked,
    /// and nothing marks where output begins or how the command ended.
    ///
    /// `C` is synthesized at the end of the echoed line, and a verdict never becomes
    /// available, so the returning prompt is the only ending such a session has to offer
    /// a listener.
    PromptAndCommandLine,
    /// `A`, `B` and `D` — the prompt region, the start of the command line, and how the
    /// command ended, with nothing marking where output begins.
    ///
    /// POSIX `sh`: `PS1` is expanded every time the prompt is drawn, and `$?` at that
    /// moment is the status of the command that just finished. Measured against busybox
    /// 1.37.0 and dash 0.5.12, `PS1='[status=$?]# '` reports `0` after `true` and `7`
    /// after `(exit 7)` in both, so the exit marker goes at the front of the prompt
    /// string and the shell fills the number in itself.
    ///
    /// What such a shell still cannot say is where output begins — there is no hook
    /// between Enter and the command running — so the tracker synthesizes `C` at the end
    /// of the echoed line exactly as it does for [`Self::PromptAndCommandLine`].
    PromptCommandLineAndExitCode,
}

impl ShellMarkers {
    pub fn marks_output_start(self) -> bool {
        matches!(self, Self::Full)
    }

    /// Whether the prompt is its own announcement rather than a block's only ending, and
    /// whether the prompt belongs in a block's content, both follow from this rather than
    /// from marking output start.
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

    #[test]
    fn a_shell_that_reports_exit_codes_need_not_mark_where_output_begins() {
        assert!(!ShellMarkers::PromptCommandLineAndExitCode.marks_output_start());
        assert!(ShellMarkers::PromptCommandLineAndExitCode.reports_exit_code());
    }

    #[test]
    fn only_the_prompt_only_shell_has_no_verdict_to_offer() {
        assert!(ShellMarkers::Full.reports_exit_code());
        assert!(!ShellMarkers::PromptAndCommandLine.reports_exit_code());
    }

    #[test]
    fn the_default_is_the_full_cycle() {
        assert_eq!(ShellMarkers::default(), ShellMarkers::Full);
    }
}
