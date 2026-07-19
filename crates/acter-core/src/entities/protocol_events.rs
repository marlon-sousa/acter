//! Entity/value: the backend-to-frontend event envelope.
//!
//! One envelope flows down the per-session Tauri Channel, so a variant needs no
//! `session_id` — the channel is the session identity. Internally tagged on `type`,
//! so specta emits a discriminated union the frontend compiler forces exhaustive
//! handling of. Producers arrive incrementally: A3's fake backend emits the command
//! trio; alt-screen, title, still-running, and connection variants are defined now
//! (both-modes protocol, implemented as a subset) and produced when their sources land.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionState, ExitCode, ReadMode};

/// Everything the backend streams to the frontend about one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// The command block opened: its output region has begun (OSC 133 C).
    CommandStarted { command_id: CommandId },
    /// A coalesced quiescent chunk of output, tagged with the read verdict to obey.
    Output {
        command_id: CommandId,
        text: String,
        read_mode: ReadMode,
    },
    /// The command block closed (OSC 133 D): exit code plus the verdict for the remainder.
    CommandFinished {
        command_id: CommandId,
        exit_code: ExitCode,
        read_mode: ReadMode,
    },
    /// Patience announcement: output has flowed for the whole window with no end marker.
    CommandStillRunning { command_id: CommandId },
    /// A program entered the alternate screen (ncurses/full-screen); interactive mode needed.
    AltScreenEntered,
    /// The alternate screen was left; non-interactive rendering resumes.
    AltScreenLeft,
    /// The terminal title changed.
    TitleChanged { title: String },
    /// The transport connection state changed.
    ConnectionChanged { state: ConnectionState },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn every_variant() -> Vec<SessionEvent> {
        vec![
            SessionEvent::CommandStarted {
                command_id: CommandId(1),
            },
            SessionEvent::Output {
                command_id: CommandId(1),
                text: "hello".to_owned(),
                read_mode: ReadMode::Auto,
            },
            SessionEvent::CommandFinished {
                command_id: CommandId(1),
                exit_code: ExitCode(0),
                read_mode: ReadMode::Quiet,
            },
            SessionEvent::CommandStillRunning {
                command_id: CommandId(1),
            },
            SessionEvent::AltScreenEntered,
            SessionEvent::AltScreenLeft,
            SessionEvent::TitleChanged {
                title: "~/acter".to_owned(),
            },
            SessionEvent::ConnectionChanged {
                state: ConnectionState::Reconnecting,
            },
        ]
    }

    #[test]
    fn every_variant_round_trips() {
        for event in every_variant() {
            let back: SessionEvent =
                serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
            assert_eq!(event, back);
        }
    }

    #[test]
    fn output_is_internally_tagged_on_type() {
        let event = SessionEvent::Output {
            command_id: CommandId(3),
            text: "line".to_owned(),
            read_mode: ReadMode::TooBig,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "Output",
                "command_id": 3,
                "text": "line",
                "read_mode": "TooBig",
            })
        );
    }

    #[test]
    fn unit_variant_carries_only_the_tag() {
        assert_eq!(
            serde_json::to_value(SessionEvent::AltScreenEntered).unwrap(),
            json!({ "type": "AltScreenEntered" })
        );
    }
}
