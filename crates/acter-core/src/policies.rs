//! Policy: pure domain computation, deterministic in → deterministic out. Facade over
//! the per-concept policy files; declares modules and re-exports their public API.

mod autoread;
mod boundary_tracker;
mod catalogue;
mod keybindings;
mod spoken;

pub(crate) use autoread::{PacingAction, on_command_end, on_output, on_wake, verdict};
pub use autoread::{TextSize, measure};
pub use boundary_tracker::{BoundaryEvent, BoundaryTracker, Region};
pub use catalogue::{Connection, catalogue};
pub use keybindings::intent_for;
pub use spoken::ended;
