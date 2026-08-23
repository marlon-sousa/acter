//! Entity/value: the domain's data types and their invariants. Facade over the
//! per-concept entity files; declares modules and re-exports their public types.

mod osc133;
mod pacing_state;
mod protocol_commands;
mod protocol_common;
mod protocol_events;
mod read_mode;
mod session_intent;
mod session_state;
mod shell_markers;
mod terminal_item;
mod unspoken_text;

pub use osc133::Osc133Marker;
pub use pacing_state::{PacingConfig, PacingState};
pub use protocol_commands::{Key, KeyAck, KeyPress, SubmitAck};
pub use protocol_common::{CommandId, ConnectionState, ExitCode, Mode, SessionId};
pub use protocol_events::{Announcement, SessionEvent};
pub(crate) use read_mode::ReadMode;
pub use session_intent::SessionIntent;
pub use session_state::{Integration, Screen, SessionState};
pub use shell_markers::ShellMarkers;
pub use terminal_item::{LineId, LineRevision, TerminalItem};
pub(crate) use unspoken_text::UnspokenText;
