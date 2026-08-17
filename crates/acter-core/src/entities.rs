//! Entity/value: the domain's data types and their invariants. Facade over the
//! per-concept entity files; declares modules and re-exports their public types.

mod osc133;
mod pacing_state;
mod protocol_commands;
mod protocol_common;
mod protocol_events;
mod session_state;
mod terminal_item;
mod unspoken_text;

pub use osc133::Osc133Marker;
pub use pacing_state::{PacingConfig, PacingState};
pub use protocol_commands::SubmitAck;
pub use protocol_common::{CommandId, ConnectionState, ExitCode, Mode, ReadMode, SessionId};
pub use protocol_events::{Announcement, SessionEvent};
pub use session_state::{Integration, Screen, SessionState};
pub use terminal_item::TerminalItem;
pub(crate) use unspoken_text::UnspokenText;
