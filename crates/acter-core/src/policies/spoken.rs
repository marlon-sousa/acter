//! Policy: making a reason that came from the world into something a screen reader can
//! read as a finished thought.

/// Trims trailing whitespace before checking for a final `.`, `!` or `?`: cmd.exe can
/// leave a trailing space, as in `set ACTER_SHELL=x && acter`.
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

    #[test]
    fn the_other_two_endings_are_endings() {
        assert_eq!(ended("Is the disk full?"), "Is the disk full?");
        assert_eq!(ended("Out of memory!"), "Out of memory!");
    }
}
