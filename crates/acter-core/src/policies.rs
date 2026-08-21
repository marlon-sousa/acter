//! Policy: pure domain computation, deterministic in → deterministic out. Facade over
//! the per-concept policy files; declares modules and re-exports their public API.

mod autoread;
mod boundary_tracker;
mod keybindings;

pub use autoread::{TextSize, measure};
pub(crate) use autoread::{PacingAction, on_command_end, on_output, on_wake, verdict};
pub use boundary_tracker::{BoundaryEvent, BoundaryTracker, Region};
pub use keybindings::intent_for;
