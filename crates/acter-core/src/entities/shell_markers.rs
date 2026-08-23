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
}

impl ShellMarkers {
    /// Whether the far end marks where a command's output begins.
    ///
    /// The question every rule that changes actually asks, so it is asked once here rather
    /// than matched on in three places.
    pub fn marks_output_start(self) -> bool {
        matches!(self, Self::Full)
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

    /// The default is what every session assumed before this type existed, so adding it
    /// changes nothing that did not opt in.
    #[test]
    fn the_default_is_the_full_cycle() {
        assert_eq!(ShellMarkers::default(), ShellMarkers::Full);
    }
}
