//! Port (driven): what the domain needs to know about one shell — how to start it, how far
//! its own command-boundary markers reach, and what ends it.

use crate::{SessionSetup, ShellMarkers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellFacts {
    /// How far this shell's markers reach once [`setup`](Self::setup) has run.
    pub markers: ShellMarkers,
    pub eof: Option<Vec<u8>>,
    pub discards_line: Option<u8>,
    /// `None` means nothing is run inside the session.
    pub setup: Option<SessionSetup>,
}

impl ShellFacts {
    pub fn of(shell: &dyn ShellAdapter) -> Self {
        Self {
            markers: shell.markers(),
            eof: shell.eof(),
            setup: shell.setup(),
            discards_line: shell.discards_line(),
        }
    }

    pub fn declined(self) -> Self {
        match self.setup {
            None => self,
            Some(_) => Self {
                markers: ShellMarkers::Full,
                setup: None,
                ..self
            },
        }
    }
}

pub trait ShellAdapter: Send + Sync {
    fn launch(&self) -> ShellLaunch;

    fn markers(&self) -> ShellMarkers;

    /// What to write when the user ends input, or `None` for a shell nobody has measured.
    fn eof(&self) -> Option<Vec<u8>>;

    /// What to run inside this shell once the session is established, or `None` for a shell
    /// nobody has written a setup for.
    fn setup(&self) -> Option<SessionSetup> {
        None
    }

    /// The byte this shell's line editor reads as "discard the pending line", or `None` for a
    /// shell that has none or that nobody has measured.
    fn discards_line(&self) -> Option<u8> {
        None
    }
}
