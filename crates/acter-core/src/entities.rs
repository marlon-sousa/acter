//! Entity/value: the domain's data types and their invariants. Facade over the
//! per-concept entity files; declares modules and re-exports their public types.

mod accepted_host_key;
mod connection_kind;
mod menu_action;
mod osc133;
mod pacing_state;
mod protocol_commands;
mod protocol_common;
mod protocol_connect;
mod protocol_events;
mod read_mode;
mod saved_connection;
mod session_intent;
mod session_setup;
mod session_state;
mod shell_install;
mod shell_markers;
mod signature_verdict;
mod stored_settings;
mod style_run;
mod terminal_item;
mod unspoken_text;

pub use accepted_host_key::{AcceptedHostKey, HostKeyOrigin};
pub use connection_kind::ConnectionKind;
pub use menu_action::MenuAction;
pub use osc133::Osc133Marker;
pub use pacing_state::{PacingConfig, PacingState};
pub use protocol_commands::{
    Connectable, Connected, Key, KeyAck, KeyPress, LaunchRequest, LineOwner, ProfileId,
    SavedConnections, SavedRow, SubmitAck, Variant, no_such_connection,
};
pub use protocol_common::{CommandId, ConnectionState, ExitCode, Mode, SessionId};
pub use protocol_connect::{AttemptId, ConnectAnswer, ConnectQuestion, ConnectStep};
pub use protocol_events::{Announcement, SessionEvent};
pub(crate) use read_mode::ReadMode;
pub use saved_connection::{SavedConnection, SavedTarget, refused, same_name};
pub use session_intent::SessionIntent;
pub use session_setup::{SessionSetup, SetUp};
pub use session_state::{Integration, Screen, SessionState};
pub use shell_install::{PathStanding, Provenance, ShellInstall};
pub use shell_markers::ShellMarkers;
pub use signature_verdict::{Fault, Signer, Verdict};
pub use stored_settings::{FORMAT, StoredSettings};
pub(crate) use style_run::utf16_len;
pub use style_run::{Colour, Style, StyleRun, join_runs, slice_runs};
pub use terminal_item::{LineId, LineRevision, TerminalItem};
pub(crate) use unspoken_text::UnspokenText;
