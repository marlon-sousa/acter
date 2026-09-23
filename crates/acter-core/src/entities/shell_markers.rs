//! Entity/value: which OSC 133 markers the shell at the far end is able to emit.

/// Which markers the far end's prompt can emit; any shell that emits `A` is `Integrated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellMarkers {
    /// Also a shell that emits nothing, which then never becomes integrated.
    #[default]
    Full,
    /// `A` and `B` only, as `cmd.exe`, which has no post-execution hook; the tracker
    /// synthesizes `C` at the end of the echoed line.
    PromptAndCommandLine,
    /// `A`, `B` and `D`, as POSIX `sh`, where `$?` in `PS1` is the last status; see
    /// acter-shells' setup.rs. The tracker synthesizes `C`.
    PromptCommandLineAndExitCode,
}

impl ShellMarkers {
    pub fn marks_output_start(self) -> bool {
        matches!(self, Self::Full)
    }

    /// Decides whether the prompt is announced on its own or left in a block's content.
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
