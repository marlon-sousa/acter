//! Policy: pure domain computation, deterministic in → deterministic out. Facade over
//! the per-concept policy files; declares modules and re-exports their public API.

mod autoread;

pub use autoread::{
    PacingAction, PacingOutcome, TextSize, measure, on_command_end, on_output, on_wake, verdict,
};
