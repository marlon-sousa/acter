//! Acter domain crate: entities, policies, ports (driven and driving), services,
//! and the IPC protocol types. No I/O and no framework dependencies live here.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod entities;
mod ports;

pub use entities::{
    CommandId, ConnectionState, ExitCode, Mode, ReadMode, SessionEvent, SessionId, SubmitAck,
};
pub use ports::{EventSink, SessionApi};
