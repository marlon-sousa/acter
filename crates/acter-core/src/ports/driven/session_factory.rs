//! Port (driven): making a session, so the domain never names a shell.
//!
//! Constructing a real far end means naming a `LocalPty`, an `AlacrittyEngine` and a
//! `SessionService`, and ARCHITECTURE allows exactly one place to name concrete
//! implementations — the composition root. So the connect service asks for a session
//! rather than building one, and the reward is that the whole of connecting is tested with
//! a fake: what is offered, what replacing means, and what happens when it fails, with no
//! process, no runtime and no Tauri anywhere near it.
//!
//! **The one driven port that starts something rather than observing it.** Every other one
//! is a seam in a session already running; this one is what makes a session exist.

use std::sync::Arc;

use crate::{ProfileId, SessionApi, SshQuestions};

/// A session that started, and what there is to say about the far end it reached.
///
/// **Two things rather than one, since B9.** Every far end before SSH was fully described
/// by the row the user chose, so a session was the whole answer. An SSH far end is asked
/// what it is *while it is being connected to* (spec B9, decision 7), and what it said is
/// known only here — so it travels back with the session rather than being reconstructed
/// by something that never spoke to the server.
pub struct Started {
    /// The session itself.
    pub session: Arc<dyn SessionApi>,
    /// One clause about this far end, to be said once at connection, or `None`.
    pub note: Option<String>,
}

/// Where a session comes from.
///
/// `Send + Sync` because the composition root hands one to a service the routers call from
/// whichever thread Tauri answers an invoke on; `&self` because a factory is asked rather
/// than driven, and an implementation that holds nothing is free to.
pub trait SessionFactory: Send + Sync {
    /// Start this profile's far end, or say why it could not be started.
    ///
    /// **The error is a whole spoken sentence, not a code and not a fragment.** It is read
    /// to somebody who has just chosen something from a list and is waiting to hear what
    /// happened, which CLAUDE.md makes a domain requirement rather than polish.
    ///
    /// **Never a panic.** Until B7 a shell that would not start took the whole application
    /// down at launch, which was defensible when the only way to name one was an
    /// environment variable set by a developer. It is not defensible when a user chooses
    /// from a menu with a working session behind them.
    /// **`questions` is how a far end that has to ask something reaches a person** (spec
    /// B9). Every far end before SSH could be started with one call: it worked, or it
    /// failed with one sentence. An SSH connection stops partway on an unknown host key or
    /// a password nobody has typed, and each is a question for whoever is in front of the
    /// window — asked through this, so no implementation of this port ever opens a dialog.
    ///
    /// Ignored by every other kind of far end, which asks nothing.
    fn open(
        &self,
        profile: &ProfileId,
        questions: &Arc<dyn SshQuestions>,
    ) -> Result<Started, String>;
}
