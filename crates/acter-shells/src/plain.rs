//! Adapter: a shell Acter knows nothing about — no arguments, no injection, and no claim
//! that the far end marks anything.
//!
//! **The null adapter, written as a type rather than as an absence.** Until B5.1 this case
//! was the `else` arm of a branch in the composition root, which meant it could only be
//! reached by not being cmd; now it can be constructed and asserted on. What it produces
//! is exactly what that arm produced, so a session over an unrecognised shell degrades
//! the way DESIGN's reliability case 2 says it should: no integration, full markers
//! assumed, and the pacing policy left to flag it once the grace period expires.

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

/// Whatever program the user named, started as it stands.
pub struct Plain {
    program: String,
}

impl Plain {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl ShellAdapter for Plain {
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: Vec::new(),
            environment: Vec::new(),
        }
    }

    /// `Full`, which is the assumption rather than a measurement: a shell nobody has
    /// integrated may or may not mark its boundaries, and believing it does is what makes
    /// a session that never marks anything reach `IntegrationUnavailable` instead of
    /// silently behaving as though a marker had been forged.
    fn markers(&self) -> ShellMarkers {
        ShellMarkers::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_acter_knows_nothing_about_is_started_as_it_stands() {
        let launch = Plain::new("nushell.exe").launch();

        assert_eq!(launch.program, "nushell.exe");
        assert!(launch.args.is_empty(), "no arguments are invented for it");
        assert!(
            launch.environment.is_empty(),
            "and nothing is injected into it"
        );
    }

    #[test]
    fn it_claims_the_markers_an_unintegrated_session_is_assumed_to_have() {
        assert_eq!(Plain::new("nushell.exe").markers(), ShellMarkers::Full);
    }
}
