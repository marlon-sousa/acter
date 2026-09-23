//! Port (driven): making a session, so the domain never names a shell.

use std::path::PathBuf;
use std::sync::Arc;

use crate::{ConnectQuestions, ProfileId, SessionApi, SetUp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub profile: ProfileId,
    /// Already resolved and verified, so an implementation starts this file and never
    /// resolves the profile's name again; `None` for a far end that is not a program on this
    /// machine.
    pub program: Option<PathBuf>,
}

pub struct Started {
    pub session: Arc<dyn SessionApi>,
    /// `None` when there is nothing to say about the far end.
    pub note: Option<String>,
    /// Whether `note` already told the listener that this session cannot say how a command
    /// went.
    pub limit_explained: bool,
}

pub trait SessionFactory: Send + Sync {
    /// `Err` is a whole spoken sentence, and an implementation never panics instead.
    fn open(
        &self,
        chosen: &Chosen,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String>;
}
