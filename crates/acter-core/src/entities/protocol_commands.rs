//! Entity/value: the payloads of the frontend-to-backend command (invoke) surface —
//! what an invoke carries in, and what it answers with.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionKind, SessionId, SetUp};

/// The immediate answer to `submit_command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status")]
pub enum SubmitAck {
    /// The id every later `CommandStarted`, `Output` and `CommandFinished` of this line carries.
    Accepted { command_id: CommandId },
    /// No session is behind this window and nothing was written; the frontend owns the sentence.
    NotConnected,
}

/// One row of the connect list: one per kind, whatever this machine has of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connectable {
    /// What starts the row when no variant is chosen; for WSL, the default distribution.
    pub id: ProfileId,
    /// Ends in `(not available)` when this machine cannot start it.
    pub label: String,
    pub available: bool,
    /// What to say about a row that cannot be connected to, and `None` when it can.
    pub instructions: Option<String>,
    /// WSL's distributions, or one entry per PowerShell install; empty when the row starts
    /// itself.
    pub variants: Vec<Variant>,
}

/// One entry in a connect-list row's panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Variant {
    pub id: ProfileId,
    /// Named without its kind: "Ubuntu", not "WSL: Ubuntu".
    pub label: String,
    /// Always true for a WSL distribution, since only installed ones can be enumerated.
    pub available: bool,
    /// What to say about a variant that cannot be started, and `None` when it can.
    pub instructions: Option<String>,
}

/// Which far end this window is on now; `use_profile` and `connected` both answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connected {
    /// Minted per connection, so a line aimed at a replaced session is refused.
    pub session: SessionId,
    pub label: String,
    /// Appended once to the connection announcement, and `None` when there is nothing to add.
    pub note: Option<String>,
    /// Whether `note` already said this session cannot report how a command went, so
    /// `IntegrationUnavailable` is not spoken again.
    pub limit_explained: bool,
    /// The saved connection this session was started from, or `None` for an unsaved one.
    pub saved_as: Option<String>,
    pub line_owner: LineOwner,
}

/// One thing that can be started: which far end, and which of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "profile")]
pub enum ProfileId {
    /// `Wsl` here means whatever distribution WSL calls the default.
    Shell { kind: ConnectionKind },
    /// One install, carrying the file discovery resolved, so the file verified is the file
    /// started.
    Install {
        kind: ConnectionKind,
        program: String,
        /// `preview`, `Microsoft Store` or the install's directory, and `None` when nothing
        /// needs telling apart.
        provenance: Option<String>,
    },
    /// Spelled as `wsl.exe -l -q` spelled it.
    Distribution { name: String },
    /// A program named by `ACTER_SHELL`.
    Program { program: String },
    Ssh {
        host: String,
        port: u16,
        user: String,
    },
    /// A built-in scenario name or a transcript path; debug builds only.
    Scripted { name: String },
}

impl ProfileId {
    /// What a listener is told this is; `(not available)` is added where the list is built.
    pub fn label(&self) -> String {
        match self {
            Self::Shell { kind } => kind.label().to_owned(),
            Self::Install {
                kind,
                provenance: None,
                ..
            } => kind.label().to_owned(),
            Self::Install {
                kind,
                provenance: Some(which),
                ..
            } => format!("{} ({which})", kind.label()),
            Self::Distribution { name } => format!("WSL: {name}"),
            // Keep in step with `SavedTarget::summary` in saved_connection.rs.
            Self::Ssh { host, port, user } => {
                if *port == 22 {
                    format!("SSH: {user} at {host}")
                } else {
                    format!("SSH: {user} at {host}, port {port}")
                }
            }
            Self::Program { program } => named(program),
            Self::Scripted { name } => format!("Scripted: {name}"),
        }
    }
}

/// The program's file name without `.exe`, trimmed because `set ACTER_SHELL=x && acter` in
/// `cmd.exe` keeps the space before `&&`.
fn named(program: &str) -> String {
    let program = program.trim();
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    file.strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".EXE"))
        .unwrap_or(file)
        .to_owned()
}

/// One keystroke the frontend did not consume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeyPress {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Only keys [`key_bytes`](crate::key_bytes) has a measured spelling for; a new variant needs
/// its measured bytes there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Tab,
    Enter,
    Backspace,
    Delete,
    Escape,
}

/// Who owns the line being edited; only the user moves it, never an inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
pub enum LineOwner {
    /// Acter's history and completion, and nothing reaches the far end until Enter.
    #[default]
    Local,
    /// Every key Acter does not consume goes to the far end as it is typed.
    FarEnd,
}

/// What became of a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KeyAck {
    Unbound,
    Applied,
    /// Bound, but no command was running, or nothing was left listening.
    NothingToActOn,
    /// Bound, but this far end has no measured bytes for it.
    Unsupported,
}

/// What the command line asked this launch to connect to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "request")]
pub enum LaunchRequest {
    /// The name as the document spells it, not as the switch did.
    Connect { name: String },
    /// Nothing is saved under this name, and [`said`](Self::Unknown::said) is what the
    /// unconnected window announces instead.
    Unknown { name: String, said: String },
}

impl LaunchRequest {
    pub fn unknown(name: &str) -> Self {
        Self::Unknown {
            name: name.to_owned(),
            said: no_such_connection(name),
        }
    }
}

pub fn no_such_connection(name: &str) -> String {
    format!("There is no saved connection named {name}.")
}

/// The saved connections resolved against this machine, as the Connect dialog lists them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SavedConnections {
    /// Sorted by name without case, and never reordered by use.
    pub rows: Vec<SavedRow>,
    /// What went wrong with a document that would not parse, and `None` when nothing did.
    pub unreadable: Option<String>,
}

/// One saved connection, as a row in that list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SavedRow {
    pub name: String,
    /// Resolved against this machine's installs, not copied from the document.
    pub id: ProfileId,
    /// Heard on arrowing onto the name: "SSH, marlon at example.org".
    pub summary: String,
    pub set_up: SetUp,
    pub line_owner: LineOwner,
    pub available: bool,
    /// What to do about a row that cannot be started, and `None` when it can.
    pub instructions: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn every_key_ack_round_trips_as_a_bare_name() {
        for ack in [
            KeyAck::Unbound,
            KeyAck::Applied,
            KeyAck::NothingToActOn,
            KeyAck::Unsupported,
        ] {
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
                available: true,
                instructions: None,
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
            note: None,
            limit_explained: false,
            saved_as: None,
            line_owner: LineOwner::FarEnd,
        };

        assert_eq!(
            serde_json::to_value(&connected).unwrap(),
            json!({
                "session": 2,
                "label": "WSL: Ubuntu",
                "note": null,
                "limit_explained": false,
                "saved_as": null,
                "line_owner": "FarEnd"
            })
        );
        let back: Connected =
            serde_json::from_value(serde_json::to_value(&connected).unwrap()).unwrap();
        assert_eq!(connected, back);
    }
}
