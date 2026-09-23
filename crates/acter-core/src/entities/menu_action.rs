//! Entity/value: the things a menu item can ask the window to do.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum MenuAction {
    /// Opens the saved connections.
    Connect,
    /// Opens the connection kinds.
    NewConnection,
    SaveConnection,
    Help,
    About,
}
