//! Acter domain crate: entities, policies, ports (driven and driving), services,
//! and the IPC protocol types. No I/O and no framework dependencies live here.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod entities;
mod policies;
mod ports;

pub use entities::{
    CommandId, ConnectionState, ExitCode, Integration, Mode, PacingConfig, PacingState, ReadMode,
    Screen, SessionEvent, SessionId, SessionState, SubmitAck,
};
pub use policies::{
    PacingAction, PacingOutcome, TextSize, measure, on_command_end, on_output, on_wake, verdict,
};
pub use ports::{EventSink, SessionApi};
