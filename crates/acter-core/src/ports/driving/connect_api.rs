//! Port (driving): what may be asked about connecting — what this machine offers, which
//! far end is behind the window now, and starting one in place of another.

use std::sync::Arc;

use crate::{
    ConnectQuestions, Connectable, Connected, LaunchRequest, ProfileId, SavedConnections, SetUp,
};

/// Every `String` in a result here, `Ok` or `Err`, is a whole sentence to speak.
pub trait ConnectApi: Send + Sync {
    /// Implementations must ask the machine on every call and never cache the list.
    fn connectable(&self) -> Vec<Connectable>;

    /// The new session is built before the old one is dropped, so an `Err` leaves the
    /// current session running and attached.
    ///
    /// Blocks until a person has answered every question, so it must never run on the thread
    /// that delivers the answers.
    ///
    /// `origin` is the saved connection this attempt started from, or `None` for a new one.
    fn use_profile(
        &self,
        id: &ProfileId,
        set_up: SetUp,
        origin: Option<&str>,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Connected, String>;

    /// `None` means the window is connected to nothing.
    fn connected(&self) -> Option<Connected>;

    fn saved(&self) -> SavedConnections;

    fn save_connection(&self, name: &str) -> Result<String, String>;

    fn rename_connection(&self, from: &str, to: &str) -> Result<String, String>;

    fn forget_connection(&self, name: &str) -> Result<String, String>;

    fn offer_to_save(&self) -> bool;

    fn stop_offering_to_save(&self) -> Result<(), String>;

    /// What `acter --connect <name>` asked for, or `None` for an ordinary launch.
    fn requested_at_launch(&self) -> Option<LaunchRequest>;
}
