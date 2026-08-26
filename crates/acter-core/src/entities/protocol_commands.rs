//! Entity/value: the payloads of the frontend-to-backend command (invoke) surface —
//! what an invoke carries in, and what it answers with.
//!
//! An invoke never waits on the shell (ARCHITECTURE, IPC rules): `submit_command`
//! returns immediately with the correlation id every later event carries — or with the
//! one refusal a session can give before it has begun, which is B7's unconnected
//! window. Phase-1
//! invoke *arguments* were all primitives (`session_id`, `line`, `cols`, `rows`) passed
//! straight to routers in A3; [`KeyPress`] is the first that is not, because a keystroke
//! is four facts that only mean anything together. Completion (A4) and session-snapshot
//! (A3) payloads are defined with the domains that build them.
//!
//! **The frontend reports the key, never the meaning** (spec B6, decision 4). What
//! `Ctrl+C` *does* is a binding, bindings are configuration, and configuration is the
//! backend's: the map from a keystroke to a
//! [`SessionIntent`](crate::SessionIntent) is `policies::keybindings`, behind this
//! seam. Only the keys the frontend did not claim for itself ever arrive here — layer 1
//! is Acter's own commands and `Ctrl+C` *with* a selection is a copy, both consumed
//! locally — so this is a short list, not every keypress crossing the IPC boundary.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionKind, SessionId};

/// The immediate answer to `submit_command`.
///
/// **Two answers rather than one since B7**, because there are now two things that can
/// become of a submitted line. A window that is not connected to anything is a state a
/// user can be in from the moment Acter opens, and a line typed into it has to be
/// *answered* rather than swallowed: silence is indistinguishable from a shell that is
/// thinking, and the text the user typed has to survive so they can connect and press
/// Enter again (spec B7, decision 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status")]
pub enum SubmitAck {
    /// Accepted: this is the id correlating this submission with its `CommandStarted` /
    /// `Output` / `CommandFinished` events.
    Accepted { command_id: CommandId },
    /// There is no session behind this window, so nothing was written anywhere.
    ///
    /// Carries no sentence: what a listener hears is the frontend's pinned string, the
    /// same one the unconnected window announced when it opened, because hearing the same
    /// words twice is how a user learns this is one state rather than two problems.
    NotConnected,
}

/// One thing a user can connect to, as [`ConnectApi::connectable`](crate::ConnectApi)
/// answers and the connect list renders it.
///
/// **Not the same value as [`Connection`](crate::Connection), and the difference is the
/// point.** B5.4's catalogue is a pure function over connection *kinds*: it decides what
/// belongs on this platform, in what order, and how a missing one reads. This is what that
/// catalogue becomes once a real machine has answered — WSL carrying the distributions it
/// actually found, the scripted sessions appended in a debug build, and every row carrying
/// the id that starts it.
///
/// **One row per kind, not one per thing that can be started** (spec A8, decision 1). The
/// dialog is a list of kinds with a panel below it holding whatever that kind needs, and a
/// listener arrows five rows rather than four plus however many distributions this machine
/// happens to have. What goes in the panel is [`variants`](Connectable::variants).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connectable {
    /// What to hand [`ConnectApi::use_profile`](crate::ConnectApi) to start this row
    /// itself, when the user has chosen no variant — which for WSL means the distribution
    /// WSL calls the default, and for every other kind is the only thing the row means.
    pub id: ProfileId,
    /// What the user hears: "Command Prompt", "PowerShell 7", "WSL", with
    /// `(not available)` on the end when this machine cannot start it.
    ///
    /// **The label belongs to the profile, not to the adapter** (spec B5.1, decision 3),
    /// which is how two rows can share one adapter — the two PowerShell editions do, and
    /// so does every WSL distribution.
    pub label: String,
    /// Whether choosing this row can start a session.
    ///
    /// A row that cannot is still listed, still focusable and still says so in its name,
    /// because a list that silently omits WSL teaches a listener that Acter does not
    /// support it (spec B5.4).
    pub available: bool,
    /// What to say about a row that cannot be connected to, and `None` when it can — a
    /// panel of instructions under a working row is noise a listener has to arrow past.
    pub instructions: Option<String>,
    /// The things *within* this kind that a user chooses between: WSL's installed
    /// distributions today, the user's saved connections of this kind with B8.
    ///
    /// Empty for a kind that is one thing — cmd is cmd — and empty for a kind this machine
    /// cannot start, because there is nothing to enumerate inside something that is not
    /// there. A row with variants starts the one the user chose; with none, it starts
    /// itself.
    pub variants: Vec<Variant>,
}

/// One thing inside a kind, as the connect dialog's panel lists it.
///
/// **Named without repeating its kind.** The row above already said WSL, and a panel that
/// reads "WSL: Ubuntu, WSL: Debian" says the same word to a listener as many times as they
/// have distributions. What [`Connected::label`] says is the full name, because a window
/// title has no row above it to lean on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Variant {
    /// What to hand [`ConnectApi::use_profile`](crate::ConnectApi) to start this one.
    pub id: ProfileId,
    /// What the user hears in the panel: "Ubuntu", not "WSL: Ubuntu".
    pub label: String,
}

/// Which far end this window is on now, and what to call it.
///
/// Returned by both `use_profile` and `connected`, because they answer the same question:
/// the first having just changed the answer, the second having merely been asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connected {
    /// The new session's id, which every later invoke about it carries.
    ///
    /// **Minted per connection rather than fixed at 1**, so a line submitted to the
    /// session the user just replaced is refused rather than run in the new one — the id
    /// finally identifies something (spec B7, decision 4).
    pub session: SessionId,
    /// What to call it: the same words the connect list used, so what a user chose and
    /// what the window then calls itself are not two different names for one thing
    /// (spec A9).
    pub label: String,
}

/// One thing that can be started: which far end, and which of it.
///
/// **A typed value rather than an opaque string**, so the factory that turns one into a
/// running session matches exhaustively and a variant cannot be added without somebody
/// deciding how to start it. It crosses the wire because the connect list is rendered by
/// the frontend and handed back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "profile")]
pub enum ProfileId {
    /// One of the catalogue's kinds, started however that kind is started.
    ///
    /// `Wsl` here is legal and means "whatever distribution WSL calls the default", which
    /// is a real session and the one `wsl.exe` with no arguments opens. The connect list
    /// never offers it, because that list can name the distributions and a user choosing
    /// by ear is better served by a name than by "the default".
    Shell { kind: ConnectionKind },
    /// Bash inside one named WSL distribution, spelled as `wsl.exe -l -q` spelled it.
    Distribution { name: String },
    /// A program named directly rather than chosen from a list: what `ACTER_SHELL` carries
    /// today, and what B8's saved profiles will carry.
    ///
    /// Started with whatever adapter recognises the name, and with none at all if nothing
    /// does — which is a session Acter supports and says nothing about.
    Program { program: String },
    /// One of the scripted far ends: a built-in name, or a path to a transcript.
    ///
    /// **Debug builds only.** A release build does not hide these — it never lists them
    /// and never constructs them (spec B7, decision 7).
    Scripted { name: String },
}

impl ProfileId {
    /// What a listener is told this is, with nothing about whether it can be started.
    ///
    /// The `(not available)` suffix is deliberately not here: whether a machine has
    /// something is not the profile's knowledge, and it is added where the list is built.
    pub fn label(&self) -> String {
        match self {
            Self::Shell { kind } => kind.label().to_owned(),
            // What it is before which one it is: "Ubuntu" on its own names no machine, and
            // a listener arrowing a list needs the category first.
            Self::Distribution { name } => format!("WSL: {name}"),
            Self::Program { program } => named(program),
            Self::Scripted { name } => format!("Scripted: {name}"),
        }
    }
}

/// The program as the user named it, without the extension a label gains nothing from.
///
/// Path and case are left alone: somebody who named a specific `pwsh.exe` by full path is
/// telling us which one they meant, and a label that quietly renamed it would be answering
/// a question they did not ask.
///
/// **Trimmed, because `cmd.exe` hands over the whitespace**: `set ACTER_SHELL=x && acter`
/// puts everything up to the `&&` into the value, trailing space included (spec A9). The
/// same trimming decides which adapter the session gets, so it happens once, here.
fn named(program: &str) -> String {
    let program = program.trim();
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    file.strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".EXE"))
        .unwrap_or(file)
        .to_owned()
}

/// One keystroke the frontend did not consume, described rather than interpreted.
///
/// Modifiers are flags rather than a set because a keystroke has exactly these three
/// and a listener never asks "which modifiers", only "was Ctrl held".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeyPress {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Which key, with exactly the one variant something presses today.
///
/// Named variants (Tab, Escape, the arrows, the function keys) arrive when an entry
/// needs them — Tab with A4's completion, the rest with phase 2's pass-through key —
/// and not before. Shipping the whole keyboard now would be a dozen variants with no
/// consumer, which is the shape B1 refused to create and B3.5 decision 10 restated
/// (spec B6, decision 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Key {
    /// A character key, as the frontend read it off the keyboard event.
    Char(char),
}

/// What became of a keystroke: the two questions the frontend cannot answer itself.
///
/// A key nothing is bound to and a bound key that found nothing running are different
/// things to say to a listener, so they are different answers here. Which words each
/// one becomes is the frontend's (A3.2); this only reports what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KeyAck {
    /// No binding for this keystroke. Nothing was attempted.
    Unbound,
    /// Bound, and acted on: the intent reached the session.
    Applied,
    /// Bound, but there was no running command to act on.
    ///
    /// The honest answer to A3.1 decision 6's "nothing to stop", which the typed `stop`
    /// had no way to give.
    NothingToActOn,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A keystroke crosses the wire, so its shape is pinned like every other IPC type.
    #[test]
    fn a_key_press_round_trips_and_shapes() {
        let press = KeyPress {
            key: Key::Char('c'),
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert_eq!(
            serde_json::to_value(&press).unwrap(),
            json!({ "key": { "Char": "c" }, "ctrl": true, "shift": false, "alt": false })
        );
        let back: KeyPress = serde_json::from_value(serde_json::to_value(&press).unwrap()).unwrap();
        assert_eq!(press, back);
    }

    /// Every answer, so a new one cannot be added without deciding what it means.
    #[test]
    fn every_key_ack_round_trips_as_a_bare_name() {
        for ack in [KeyAck::Unbound, KeyAck::Applied, KeyAck::NothingToActOn] {
            let json = serde_json::to_value(ack).unwrap();
            assert!(json.is_string(), "a unit answer is a bare name: {json}");
            let back: KeyAck = serde_json::from_value(json).unwrap();
            assert_eq!(ack, back);
        }
    }

    #[test]
    fn submit_ack_round_trips_and_shapes() {
        let ack = SubmitAck::Accepted {
            command_id: CommandId(9),
        };
        assert_eq!(
            serde_json::to_value(&ack).unwrap(),
            json!({ "status": "Accepted", "command_id": 9 })
        );
        let back: SubmitAck = serde_json::from_value(serde_json::to_value(&ack).unwrap()).unwrap();
        assert_eq!(ack, back);
    }

    /// The refusal is a shape of its own on the wire rather than a missing id, so a
    /// frontend cannot read "not connected" as "command zero" and open a block for it.
    #[test]
    fn a_refused_submission_round_trips_as_its_own_shape() {
        let ack = SubmitAck::NotConnected;

        assert_eq!(
            serde_json::to_value(&ack).unwrap(),
            json!({ "status": "NotConnected" })
        );
        let back: SubmitAck = serde_json::from_value(serde_json::to_value(&ack).unwrap()).unwrap();
        assert_eq!(ack, back);
    }

    /// Every profile shape crosses the wire and comes back the same thing. The list is
    /// exhaustive on purpose: a variant added without a way to start it is the failure
    /// this pins.
    #[test]
    fn every_profile_id_round_trips() {
        for id in [
            ProfileId::Shell {
                kind: ConnectionKind::Cmd,
            },
            ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            },
            ProfileId::Distribution {
                name: "Ubuntu".to_owned(),
            },
            ProfileId::Program {
                program: "nushell.exe".to_owned(),
            },
            ProfileId::Scripted {
                name: "builtin".to_owned(),
            },
        ] {
            let back: ProfileId =
                serde_json::from_value(serde_json::to_value(&id).unwrap()).unwrap();
            assert_eq!(id, back);
        }
    }

    /// What a listener hears for each kind of profile. A distribution says what it is
    /// before it says which one, because "Ubuntu" on its own names no machine.
    #[test]
    fn every_profile_says_what_it_is() {
        assert_eq!(
            ProfileId::Shell {
                kind: ConnectionKind::PowerShellSeven
            }
            .label(),
            "PowerShell 7"
        );
        assert_eq!(
            ProfileId::Distribution {
                name: "Ubuntu".to_owned()
            }
            .label(),
            "WSL: Ubuntu"
        );
        assert_eq!(
            ProfileId::Scripted {
                name: "builtin".to_owned()
            }
            .label(),
            "Scripted: builtin"
        );
    }

    /// A program is called what the user called it, minus the extension and minus the
    /// whitespace `cmd.exe` puts on the end of an environment variable — the same trimming
    /// A9 needed for the window title, in the one place that now decides it.
    #[test]
    fn a_program_is_labelled_by_its_name_alone() {
        for (program, expected) in [
            ("powershell.exe", "powershell"),
            ("  powershell.exe ", "powershell"),
            (r"C:\Windows\system32\cmd.exe", "cmd"),
            ("pwsh", "pwsh"),
            ("my.shell", "my.shell"),
        ] {
            assert_eq!(
                ProfileId::Program {
                    program: program.to_owned()
                }
                .label(),
                expected
            );
        }
    }

    /// A row of the connect list crosses the wire whole, instructions included: the panel
    /// under an unavailable row is rendered from this rather than from a kind the frontend
    /// would have to map for itself.
    #[test]
    fn a_connectable_row_round_trips_with_what_to_do_about_it() {
        let row = Connectable {
            id: ProfileId::Shell {
                kind: ConnectionKind::PowerShellSeven,
            },
            label: "PowerShell 7 (not available)".to_owned(),
            available: false,
            instructions: Some(ConnectionKind::PowerShellSeven.instructions().to_owned()),
            variants: Vec::new(),
        };

        let back: Connectable =
            serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
        assert_eq!(row, back);
    }

    /// A row with variants carries them, and they are named without repeating the kind the
    /// row above already said (spec A8, decision 1).
    #[test]
    fn a_row_with_variants_carries_them_named_for_the_panel() {
        let row = Connectable {
            id: ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            },
            label: "WSL".to_owned(),
            available: true,
            instructions: None,
            variants: vec![Variant {
                id: ProfileId::Distribution {
                    name: "Ubuntu".to_owned(),
                },
                label: "Ubuntu".to_owned(),
            }],
        };

        let back: Connectable =
            serde_json::from_value(serde_json::to_value(&row).unwrap()).unwrap();
        assert_eq!(row, back);
        assert_eq!(
            row.variants[0].label, "Ubuntu",
            "the panel says the distribution"
        );
        assert_eq!(
            row.variants[0].id.label(),
            "WSL: Ubuntu",
            "and the window title says which kind it is, having no row above it"
        );
    }

    #[test]
    fn a_connection_round_trips_with_the_id_every_later_invoke_carries() {
        let connected = Connected {
            session: SessionId(2),
            label: "WSL: Ubuntu".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&connected).unwrap(),
            json!({ "session": 2, "label": "WSL: Ubuntu" })
        );
        let back: Connected =
            serde_json::from_value(serde_json::to_value(&connected).unwrap()).unwrap();
        assert_eq!(connected, back);
    }
}
