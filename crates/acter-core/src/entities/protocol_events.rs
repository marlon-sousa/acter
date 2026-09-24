//! Entity/value: the backend-to-frontend event envelope.
//!
//! Each session has its own Tauri Channel, so no variant carries a session id.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionState, ExitCode, LineId, LineRevision};

/// Everything the backend streams to the frontend about one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// OSC 133 C. `command_line` is what the shell echoed between B and C, and `None` when
    /// there was no echo or it could not be told apart from the prompt.
    CommandStarted {
        command_id: CommandId,
        command_line: Option<String>,
    },
    /// What to put in the buffer, never what to say; speech is only ever an
    /// [`Announce`](SessionEvent::Announce). `prompt` marks a row the shell drew as its prompt.
    Output {
        command_id: CommandId,
        line: LineId,
        revision: LineRevision,
        text: String,
        prompt: bool,
    },
    /// OSC 133 D. A nonzero exit code follows as `Announce { Failed }`; a zero one is never
    /// sent.
    CommandFinished {
        command_id: CommandId,
    },
    /// Emitted only when the shell reports exit codes; otherwise the prompt already arrives as
    /// output.
    PromptDrawn {
        text: String,
    },
    /// The user stopped the command; no `CommandFinished` follows.
    CommandInterrupted {
        command_id: CommandId,
    },
    /// The startup grace period passed with no shell-integration marker; a marker arriving
    /// later upgrades the session silently.
    IntegrationUnavailable,
    AltScreenEntered,
    AltScreenLeft,
    TitleChanged {
        title: String,
    },
    /// The far end's command line in far-end-line mode; `text` is `None` when only the caret
    /// moved, and `caret` counts characters from the anchor column.
    FarEndLine {
        text: Option<String>,
        caret: u32,
        /// False when `text` is a whole row the far end changed rather than its command line.
        anchored: bool,
    },
    ConnectionChanged {
        state: ConnectionState,
    },
    /// Always sent after the `Output` it speaks about, on the same in-order channel, so
    /// spoken text is already in the buffer.
    Announce {
        command_id: CommandId,
        announcement: Announcement,
    },
}

/// What to say. Only `ReadAloud` carries words; the frontend owns every other sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Announcement {
    ReadAloud {
        text: String,
    },
    /// Past the auto-read threshold, announced by line count.
    TooBig {
        lines: u32,
    },
    /// The patience window elapsed with output still flowing.
    StillRunning,
    /// The babble guard tripped: output keeps arriving in the buffer, unannounced.
    OutputContinues,
    /// Nonzero exit code.
    Failed {
        exit_code: ExitCode,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn every_variant() -> Vec<SessionEvent> {
        vec![
            SessionEvent::CommandStarted {
                command_id: CommandId(1),
                command_line: Some("git status".to_owned()),
            },
            SessionEvent::CommandStarted {
                command_id: CommandId(1),
                command_line: None,
            },
            SessionEvent::Output {
                command_id: CommandId(1),
                line: LineId(4),
                revision: LineRevision::Appended,
                text: "hello".to_owned(),
                prompt: false,
            },
            SessionEvent::Output {
                command_id: CommandId(1),
                line: LineId(4),
                revision: LineRevision::Rewritten,
                text: "hello again".to_owned(),
                prompt: false,
            },
            SessionEvent::CommandFinished {
                command_id: CommandId(1),
            },
            SessionEvent::CommandInterrupted {
                command_id: CommandId(1),
            },
            SessionEvent::IntegrationUnavailable,
            SessionEvent::AltScreenEntered,
            SessionEvent::AltScreenLeft,
            SessionEvent::TitleChanged {
                title: "~/acter".to_owned(),
            },
            SessionEvent::FarEndLine {
                text: Some("cargo test --all".to_owned()),
                caret: 16,
                anchored: true,
            },
            SessionEvent::FarEndLine {
                text: None,
                caret: 3,
                anchored: true,
            },
            SessionEvent::FarEndLine {
                text: Some("> Skip pushing the branch".to_owned()),
                caret: 0,
                anchored: false,
            },
            SessionEvent::ConnectionChanged {
                state: ConnectionState::Reconnecting,
            },
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::ReadAloud {
                    text: "hello".to_owned(),
                },
            },
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::TooBig { lines: 120 },
            },
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::StillRunning,
            },
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::OutputContinues,
            },
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::Failed {
                    exit_code: ExitCode(1),
                },
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
            line: LineId(9),
            revision: LineRevision::Appended,
            text: "line".to_owned(),
            prompt: false,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "Output",
                "command_id": 3,
                "line": 9,
                "revision": "Appended",
                "text": "line",
                "prompt": false,
            })
        );
    }

    #[test]
    fn a_rewrite_names_the_line_it_replaces() {
        assert_eq!(
            serde_json::to_value(SessionEvent::Output {
                command_id: CommandId(3),
                line: LineId(9),
                revision: LineRevision::Rewritten,
                text: String::new(),
                prompt: false,
            })
            .unwrap(),
            json!({
                "type": "Output",
                "command_id": 3,
                "line": 9,
                "revision": "Rewritten",
                "text": "",
                "prompt": false,
            })
        );
    }

    #[test]
    fn the_far_end_line_carries_a_row_and_a_caret() {
        assert_eq!(
            serde_json::to_value(SessionEvent::FarEndLine {
                text: Some("exit".to_owned()),
                caret: 4,
                anchored: true,
            })
            .unwrap(),
            json!({ "type": "FarEndLine", "text": "exit", "caret": 4, "anchored": true })
        );
        assert_eq!(
            serde_json::to_value(SessionEvent::FarEndLine {
                text: None,
                caret: 2,
                anchored: false,
            })
            .unwrap(),
            json!({ "type": "FarEndLine", "text": null, "caret": 2, "anchored": false })
        );
    }

    #[test]
    fn an_announcement_is_a_nested_tagged_object() {
        assert_eq!(
            serde_json::to_value(SessionEvent::Announce {
                command_id: CommandId(7),
                announcement: Announcement::TooBig { lines: 120 },
            })
            .unwrap(),
            json!({
                "type": "Announce",
                "command_id": 7,
                "announcement": { "kind": "TooBig", "lines": 120 },
            })
        );
    }

    #[test]
    fn an_unknown_command_line_is_null() {
        assert_eq!(
            serde_json::to_value(SessionEvent::CommandStarted {
                command_id: CommandId(4),
                command_line: None,
            })
            .unwrap(),
            json!({
                "type": "CommandStarted",
                "command_id": 4,
                "command_line": null,
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
