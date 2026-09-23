//! Adapter: a shell Acter knows nothing about — no arguments, no injection, and no claim
//! that the far end marks anything.

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

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

    /// An assumption, not a measurement: it is what lets a session that never marks anything
    /// reach `IntegrationUnavailable`.
    fn markers(&self) -> ShellMarkers {
        ShellMarkers::Full
    }

    fn eof(&self) -> Option<Vec<u8>> {
        None
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
