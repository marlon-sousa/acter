//! Entity/value: the domain's data types and their invariants. Facade over the
//! per-concept entity files; declares modules and re-exports their public types.

mod protocol_commands;
mod protocol_common;
mod protocol_events;

pub use protocol_commands::SubmitAck;
pub use protocol_common::{CommandId, ConnectionState, ExitCode, Mode, ReadMode, SessionId};
pub use protocol_events::SessionEvent;
