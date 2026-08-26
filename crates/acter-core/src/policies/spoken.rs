//! Policy: making a reason that came from the world into something a screen reader can
//! read as a finished thought.
//!
//! **The world does not punctuate.** A missing transcript ends `(os error 2)`, a
//! pseudoconsole that will not open ends with whatever the library said, and a file that
//! could not be written ends with the operating system's own phrasing — so a reader runs
//! straight on from the end of the explanation into whatever it says next, with no pause
//! where the thought finished. Every user-facing string in this product is read aloud,
//! which makes this a domain requirement rather than typography (CLAUDE.md).
//!
//! **Here rather than in the composition root, since B9.** It was written for the shell
//! that would not start and lived beside that one caller; SSH gives it a second, in another
//! crate — a host key that was accepted and could not be written down. A rule about how
//! this product speaks, applied in two crates, is a policy.

/// One reason, ended as a sentence, and left alone when it already is one.
///
/// Trailing whitespace goes with it: `cmd.exe` hands over the space in
/// `set ACTER_SHELL=x && acter`, and a sentence that ends in a space before its full stop
/// is a sentence a reader pauses in the wrong place in.
pub fn ended(reason: impl Into<String>) -> String {
    let reason = reason.into();
    let trimmed = reason.trim_end();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The world does not punctuate, and this is what a listener meets when it does not.**
    /// A missing transcript ends `(os error 2)`; without this the sentence about it ran
    /// straight on into whatever the reader said next, with no pause where the thought
    /// finished.
    #[test]
    fn a_reason_the_world_wrote_is_ended_before_it_is_spoken() {
        assert_eq!(
            ended("The system cannot find the file specified. (os error 2)"),
            "The system cannot find the file specified. (os error 2)."
        );
    }

    #[test]
    fn a_reason_that_already_ends_is_left_alone() {
        assert_eq!(ended("Access is denied."), "Access is denied.");
        assert_eq!(
            ended("Access is denied.  "),
            "Access is denied.",
            "including one that ends in trailing space"
        );
    }

    /// A question and an exclamation are endings too — rare from an operating system, and
    /// the alternative is a full stop after a question mark.
    #[test]
    fn the_other_two_endings_are_endings() {
        assert_eq!(ended("Is the disk full?"), "Is the disk full?");
        assert_eq!(ended("Out of memory!"), "Out of memory!");
    }
}
