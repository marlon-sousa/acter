//! Acter domain crate: entities, policies, ports (driven and driving), services,
//! and the IPC protocol types. No I/O and no framework dependencies live here.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod controllers;
mod entities;
mod policies;
mod ports;
mod services;

pub use controllers::{Requests, SessionActor, SessionInput, Wake};
pub use entities::{
    Announcement, CommandId, ConnectionKind, ConnectionState, ExitCode, Integration, Key, KeyAck,
    KeyPress, LineId, LineRevision, Mode, Osc133Marker, PacingConfig, PacingState, Screen,
    SessionEvent, SessionId, SessionIntent, SessionState, ShellMarkers, SubmitAck, TerminalItem,
};
// The pacing verdict is domain-internal since A6: `ReadMode` no longer crosses the wire,
// so the items whose signatures mention it — `PacingAction`, `PacingOutcome`, `verdict`
// and the three transition functions — are `pub(crate)` in `policies` and reached
// through that module rather than re-exported here. Nothing outside this crate used them.
pub use policies::{
    BoundaryEvent, BoundaryTracker, Connection, Region, TextSize, catalogue, intent_for, measure,
};
pub use ports::{
    Clock, EventSink, InstalledShells, NoDistributions, SessionApi, ShellAdapter, ShellFacts,
    ShellLaunch, TerminalEngine, Timer, Transport, TransportError,
};
pub use services::SessionService;
