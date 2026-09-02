//! Entity/value: the backend-to-frontend event envelope.
//!
//! One envelope flows down the per-session Tauri Channel, so a variant needs no
//! `session_id` — the channel is the session identity. Internally tagged on `type`,
//! so specta emits a discriminated union the frontend compiler forces exhaustive
//! handling of. Producers arrive incrementally: the command trio, the alt-screen pair
//! and the announcement event have producers; title and connection variants are defined
//! now (both-modes protocol, implemented as a subset) and produced when their sources
//! land.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionState, ExitCode, LineId, LineRevision};

/// Everything the backend streams to the frontend about one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// The command block opened: its output region has begun (OSC 133 C).
    ///
    /// `command_line` is what the shell echoed for this block (the B..C region), which
    /// is the shell itself saying which line it read. The frontend prefers it over the
    /// optimistic heading it put on the block when the submission was acked, so an id
    /// that drifted can no longer put the wrong words on a block (spec B6.1, decision 1).
    ///
    /// `None` is a real state and not a missing value: an unintegrated session has no
    /// B..C region at all, a shell may emit `C` with nothing echoed before it, and an
    /// echo the service could not read apart from the prompt it was written after is
    /// deliberately reported as unknown. The frontend's answer to `None` is to keep the
    /// heading it has.
    CommandStarted {
        command_id: CommandId,
        command_line: Option<String>,
    },
    /// One line of output, and what this event does to it. Rendering only: it says what to
    /// put in the buffer and never what to say about it. Whether any of it is spoken is a
    /// separate [`Announce`](SessionEvent::Announce) (A6).
    ///
    /// **It names a line since 28** (decision 8), and that is what makes the transcript
    /// honest. A terminal's output is not append-only: `readline` repaints the row it is
    /// editing, and `gh` blanks its option rows when the prompt is answered — so a buffer
    /// that could only append grew a junk line per arrow press and kept three option rows
    /// the far end had already erased. With the id and the revision the buffer assigns or
    /// appends by line, blanks included, and the far end writes its own record.
    ///
    /// This is DESIGN's separate-paths decision unchanged rather than widened: the buffer
    /// applies all three revisions, and what is *spoken* is still an
    /// [`Announce`](SessionEvent::Announce) about text that is already there — a rewrite
    /// reaches the buffer and never the speech path.
    Output {
        command_id: CommandId,
        line: LineId,
        revision: LineRevision,
        text: String,
    },
    /// The command block closed (OSC 133 D).
    ///
    /// Carries no exit code. A nonzero one arrives as `Announce { Failed }`, after the
    /// remainder of the output, which is the order a listener needs: the error text
    /// first, the verdict about it second. A successful command's code is therefore not
    /// on the wire at all — nothing read it, and the frontend must not speak it (A6
    /// decision 2). A later feature wanting the code adds a shape for it deliberately.
    CommandFinished { command_id: CommandId },
    /// The shell drew a prompt, and this is what it says.
    ///
    /// **Restores what shell integration took away** (spec B5.6). The prompt is where a
    /// terminal user reads their working directory, their git branch, their virtualenv —
    /// and in a session marking all four boundaries it lives in the `A..B` region, which
    /// block content excludes, so a listener heard it nowhere at all. `D` replaced the
    /// prompt as an *ending signal* and replaced nothing about what it *says*.
    ///
    /// **Its own event rather than block content.** A prompt admitted as output would
    /// arrive inside a block, before that block's verdict, reading as though the shell had
    /// printed it. It is not output: it is the state the next command will run in.
    ///
    /// Only a session whose shell reports an exit code emits it. A shell with no `D` already
    /// speaks the prompt as content, because the returning prompt is the only ending it has
    /// (spec B4.5, decision 4) — emitting this as well would say everything twice.
    ///
    /// **The condition is the verdict rather than the full marker cycle** (roadmap 23.15).
    /// It read `ShellMarkers::Full` while that was the only shell in the product with a `D`;
    /// a POSIX `sh` that reports exit codes has one and marks no `C`, and its prompt is news
    /// for the same reason bash's is.
    PromptDrawn { text: String },
    /// The command was stopped before it ended on its own. Terminal: no
    /// `CommandFinished` follows. Distinct from `CommandFinished` on purpose — the exit
    /// code of a process the user stopped carries no information worth announcing, and
    /// inferring "stopped" from a conventional code (130 on Unix, `0xC000013A` on
    /// Windows) would mis-announce a program that genuinely exits with it.
    CommandInterrupted { command_id: CommandId },
    /// The startup grace period elapsed with no shell-integration markers: this session
    /// has no command boundaries and every command in it degrades to patience-only
    /// behavior (DESIGN's reliability case 2).
    ///
    /// Session-scoped and carrying no `command_id`, the shape `AltScreenEntered` and
    /// `AltScreenLeft` already have, because it fires at session start before any
    /// command exists — which is why it is not an
    /// [`Announce`](SessionEvent::Announce), whose payload is about a command. Reusing
    /// `ConnectionChanged` was rejected: that describes the transport, and "the pipe is
    /// down" and "the shell did not announce itself" must not sound alike to a listener
    /// (spec B6, decision 11).
    ///
    /// Recovery is silent: a marker arriving later upgrades the session (DESIGN
    /// decision 8) and nothing is said, because there is nothing the user must do
    /// differently.
    IntegrationUnavailable,
    /// A program entered the alternate screen (ncurses/full-screen); interactive mode needed.
    AltScreenEntered,
    /// The alternate screen was left; non-interactive rendering resumes.
    AltScreenLeft,
    /// The terminal title changed.
    TitleChanged { title: String },
    /// What the far end's command line says now, and where its cursor is in it.
    ///
    /// **Only in far-end-line mode, and it carries no words of Acter's** (spec 28,
    /// decisions 2 and 3). The frontend writes both into an ARIA text box — a
    /// `contenteditable` span with `role="textbox"` — and the reader does the speaking out
    /// of its own text-box behaviour: the row when the row changed, the character at the
    /// caret when only the cursor moved, and "blank" for a row a key emptied or a caret
    /// past the end. That is why there is no live region on this path and no string here:
    /// what a listener hears is identical in kind to what they hear in every other text box
    /// on Windows.
    ///
    /// `text` is `None` when nothing was redrawn and only the caret moved, which is the
    /// whole of what left, right, Home and End do — they rewrite nothing, so there is no
    /// text to send and a row replaced with itself would be a change the reader announces.
    ///
    /// `caret` counts characters from the start of the text the field holds, which is the
    /// anchored row from the anchor column onward.
    FarEndLine { text: Option<String>, caret: u32 },
    /// The transport connection state changed.
    ConnectionChanged { state: ConnectionState },
    /// Something should be said. Speaking is its own event: as long as one event type
    /// could mean both "render this" and "say this", the two paths DESIGN separates stay
    /// coupled in the type system. The frontend appends [`SessionEvent::Output`] to the
    /// buffer and routes this to the announcer, with no branching between them.
    ///
    /// Ordering carries the invariant: the actor emits the rendering event covering a
    /// span before any `Announce` about it, and the per-session channel delivers in
    /// order, so text is always in the buffer before it is spoken. Nothing correlates
    /// the two — an announcement is self-contained, which is why a rendered span being
    /// evicted under the frontend's line cap can never silence one.
    Announce {
        command_id: CommandId,
        announcement: Announcement,
    },
}

/// What to say. Only reading output aloud carries text; every other announcement
/// carries the data the frontend needs to build a pinned string it already owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Announcement {
    /// Read this span aloud: it is under the auto-read threshold.
    ReadAloud { text: String },
    /// Over the threshold — announced by size, not read. Carries no text on purpose:
    /// the span is already rendered, and past the threshold the actor stops holding it.
    TooBig { lines: u32 },
    /// The patience window elapsed with output still flowing.
    StillRunning,
    /// The babble guard tripped: output keeps arriving in the buffer, unannounced.
    OutputContinues,
    /// The command ended with a nonzero exit code.
    Failed { exit_code: ExitCode },
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
            },
            SessionEvent::Output {
                command_id: CommandId(1),
                line: LineId(4),
                revision: LineRevision::Rewritten,
                text: "hello again".to_owned(),
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
            },
            SessionEvent::FarEndLine {
                text: None,
                caret: 3,
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
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "Output",
                "command_id": 3,
                "line": 9,
                "revision": "Appended",
                "text": "line",
            })
        );
    }

    /// The id is a bare number on the wire, like every other identifier this protocol
    /// carries, and the revision is a bare name — so a frontend applying one reads what the
    /// engine decided rather than re-deriving it from the text.
    #[test]
    fn a_rewrite_names_the_line_it_replaces() {
        assert_eq!(
            serde_json::to_value(SessionEvent::Output {
                command_id: CommandId(3),
                line: LineId(9),
                revision: LineRevision::Rewritten,
                text: String::new(),
            })
            .unwrap(),
            json!({
                "type": "Output",
                "command_id": 3,
                "line": 9,
                "revision": "Rewritten",
                "text": "",
            })
        );
    }

    /// **The far-end line carries a row and a caret and no words at all**, because the
    /// reader speaks it: `text` absent is "only the caret moved", which is what left and
    /// right do.
    #[test]
    fn the_far_end_line_carries_a_row_and_a_caret() {
        assert_eq!(
            serde_json::to_value(SessionEvent::FarEndLine {
                text: Some("exit".to_owned()),
                caret: 4,
            })
            .unwrap(),
            json!({ "type": "FarEndLine", "text": "exit", "caret": 4 })
        );
        assert_eq!(
            serde_json::to_value(SessionEvent::FarEndLine {
                text: None,
                caret: 2,
            })
            .unwrap(),
            json!({ "type": "FarEndLine", "text": null, "caret": 2 })
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

    /// An echo the service could not read is `null` on the wire rather than an absent
    /// field or an empty string: the frontend branches on it, and "the shell did not tell
    /// us" has to be distinguishable from "the shell echoed nothing at all".
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
