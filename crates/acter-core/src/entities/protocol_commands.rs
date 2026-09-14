//! Entity/value: the payloads of the frontend-to-backend command (invoke) surface —
//! what an invoke carries in, and what it answers with.
//!
//! An invoke never waits on the shell: `submit_command` returns immediately with the
//! correlation id every later event carries, or with the one refusal a session can give
//! before it has begun, an unconnected window.
//!
//! The frontend reports the key, never the meaning: what `Ctrl+C` *does* is a binding,
//! bindings are configuration, and configuration is the backend's, behind this seam. Only
//! the keys the frontend did not claim for itself ever arrive here — layer 1 is Acter's
//! own commands and `Ctrl+C` *with* a selection is a copy, both consumed locally — so
//! this is a short list, not every keypress crossing the IPC boundary.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandId, ConnectionKind, SessionId, SetUp};

/// The immediate answer to `submit_command`.
///
/// Two answers rather than one: a window that is not connected to anything is a state a
/// user can be in from the moment Acter opens, and a line typed into it has to be
/// *answered* rather than swallowed — silence is indistinguishable from a shell that is
/// thinking, and the text the user typed has to survive so they can connect and press
/// Enter again.
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
/// answers and the connect list renders it. Not the same value as
/// [`Connection`](crate::Connection), which decides what belongs on this platform: this
/// is what that becomes once a real machine has answered.
///
/// One row per kind, not one per thing that can be started, so a listener arrows five
/// rows rather than four plus however many distributions this machine happens to have.
/// What goes in the panel is [`variants`](Connectable::variants).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connectable {
    /// What to hand [`ConnectApi::use_profile`](crate::ConnectApi) to start this row
    /// itself, when the user has chosen no variant — which for WSL means the distribution
    /// WSL calls the default, and for every other kind is the only thing the row means.
    pub id: ProfileId,
    /// What the user hears: "Command Prompt", "PowerShell 7", "WSL", with
    /// `(not available)` on the end when this machine cannot start it.
    pub label: String,
    /// Whether choosing this row can start a session. A row that cannot is still listed,
    /// still focusable and still says so in its name.
    pub available: bool,
    /// What to say about a row that cannot be connected to, and `None` when it can.
    pub instructions: Option<String>,
    /// The things *within* this kind that a user chooses between: WSL's installed
    /// distributions today, the user's saved connections of this kind with B8. A row with
    /// variants starts the one the user chose; with none, it starts itself.
    pub variants: Vec<Variant>,
}

/// One thing inside a kind, as the connect dialog's panel lists it. Named without
/// repeating its kind: the row above already said WSL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Variant {
    /// What to hand [`ConnectApi::use_profile`](crate::ConnectApi) to start this one.
    pub id: ProfileId,
    /// What the user hears in the panel: "Ubuntu", not "WSL: Ubuntu", with
    /// `(not available)` on the end when this machine cannot start it.
    pub label: String,
    /// Whether choosing this one can start a session. A variant can be missing while its
    /// kind is not: a machine with Windows PowerShell and no PowerShell 7 has the kind
    /// and one of its two editions, and the missing one stays listed.
    ///
    /// Always true for a WSL distribution: distributions are *discovered*, so one that
    /// is not installed cannot be enumerated and has no name to list.
    pub available: bool,
    /// What to say about a variant that cannot be started, and `None` when it can.
    pub instructions: Option<String>,
}

/// Which far end this window is on now, and what to call it.
///
/// Returned by both `use_profile` and `connected`, because they answer the same question:
/// the first having just changed the answer, the second having merely been asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Connected {
    /// The new session's id, which every later invoke about it carries.
    ///
    /// Minted per connection rather than fixed at 1, so a line submitted to the session
    /// the user just replaced is refused rather than run in the new one.
    pub session: SessionId,
    /// What to call it: the same words the connect list used, so what a user chose and
    /// what the window then calls itself are not two different names for one thing.
    pub label: String,
    /// What else there is to say about this far end, once, at connection — and `None`
    /// when there is nothing. Lets a listener hear one sentence rather than two: an SSH
    /// session that is unintegrated says so and also says *what it is*, and the frontend
    /// appends this to the connection announcement and suppresses the session's own one.
    pub note: Option<String>,
    /// Whether that note already told the listener that this session cannot say how a
    /// command went, so the session's own `IntegrationUnavailable` is not said a second
    /// time. Computed by the one function that composes the note, so the two cannot
    /// disagree.
    pub limit_explained: bool,
    /// The saved connection this session was started from, or `None` for one nobody has
    /// named yet. The frontend's own knowledge travelling back, since the user may have
    /// edited the panel before pressing Connect.
    pub saved_as: Option<String>,
    /// Who holds the line as this session opens: what the saved connection asked for, and
    /// the default otherwise.
    pub line_owner: LineOwner,
}

/// One thing that can be started: which far end, and which of it.
///
/// A typed value rather than an opaque string, so the factory that turns one into a
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
    /// One *particular* install of a kind: which edition it is, and the file that is it.
    /// [`Self::Shell`] leaves the file to whatever `PATH` resolves at spawn time; this
    /// carries the file the list already resolved, so what is verified and what is
    /// started are the same bytes.
    Install {
        /// Which edition this is, so a session started from it says what it says today.
        kind: ConnectionKind,
        /// The file, resolved once at discovery.
        program: String,
        /// What tells this install from another of the same edition, when anything does:
        /// `preview`, `Microsoft Store`, or the directory it lives in.
        ///
        /// `None` on the ordinary machine with one install.
        provenance: Option<String>,
    },
    /// Bash inside one named WSL distribution, spelled as `wsl.exe -l -q` spelled it.
    Distribution { name: String },
    /// A program named directly rather than chosen from a list: what `ACTER_SHELL` carries
    /// today.
    ///
    /// Started with whatever adapter recognises the name, and with none at all if nothing
    /// does — which is a session Acter supports and says nothing about.
    Program { program: String },
    /// A machine that is not this one, reached over SSH. Three fields rather than a
    /// string to parse: `user@host:port` is a spelling, and a spelling can be got wrong.
    /// Nothing is stored here: a password is asked for every time and never written down.
    Ssh {
        host: String,
        port: u16,
        user: String,
    },
    /// One of the scripted far ends: a built-in name, or a path to a transcript.
    ///
    /// Debug builds only. A release build does not hide these: it never lists them and
    /// never constructs them.
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
            // The provenance is in the name rather than a version number, because no
            // version can be read off the file reliably, and it is absent on the machine
            // with one install, so that machine hears exactly what it hears today.
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
            // What it is before which one it is: "Ubuntu" on its own names no machine, and
            // a listener arrowing a list needs the category first.
            Self::Distribution { name } => format!("WSL: {name}"),
            // Same ordering rule as `SavedTarget::summary` in saved_connection.rs: the
            // account before the machine, and the port only when it is not 22.
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

/// The program as the user named it, without the extension a label gains nothing from.
///
/// Path and case are left alone: somebody who named a specific `pwsh.exe` by full path is
/// telling us which one they meant, and a label that quietly renamed it would be answering
/// a question they did not ask.
///
/// Trimmed, because `cmd.exe` hands over the whitespace: `set ACTER_SHELL=x && acter`
/// puts everything up to the `&&` into the value, trailing space included. The same
/// trimming decides which adapter the session gets, so it happens once, here.
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

/// Which key, as the frontend read it off the keyboard event.
///
/// `Char` alone would do for local editing, but a key aimed at the far end has to be
/// spelled as bytes, and an arrow is not a character.
///
/// The list is what [`key_bytes`](crate::key_bytes) has a measured spelling for and stops
/// there. The function keys are still absent, and so is every key nobody has measured a far
/// end's answer to: a variant with a guessed spelling is worse than no variant, because the
/// far end simply does something else and says nothing about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Key {
    /// A character key, as the frontend read it off the keyboard event.
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

/// Who owns the line being edited: Acter, or the far end. A state rather than a setting,
/// and never inferred: a Tab that completes against Acter's history while the far end
/// holds its own line buffer corrupts a command line, so ownership moves whole or not at
/// all, and the user is the only thing that moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
pub enum LineOwner {
    /// Acter owns the line: its own history, its own completion, and nothing crosses to the
    /// far end until Enter. The default in every session.
    #[default]
    Local,
    /// The far end owns the line: every key that is not layer 1 goes to it as it is typed,
    /// and what the user hears back is the row the far end redrew.
    FarEnd,
}

/// What became of a keystroke: the two questions the frontend cannot answer itself.
///
/// A key nothing is bound to and a bound key that found nothing running are different
/// things to say to a listener, so they are different answers here. Which words each
/// one becomes is the frontend's; this only reports what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KeyAck {
    /// No binding for this keystroke. Nothing was attempted.
    Unbound,
    /// Bound, and acted on: the intent reached the session.
    Applied,
    /// Bound, but there was no running command to act on — and, for a key aimed at the far
    /// end rather than at a command, nothing left listening at all.
    NothingToActOn,
    /// Bound, and this far end has no measured answer for it.
    ///
    /// Different from [`Self::NothingToActOn`]: "this shell has no key for end of input"
    /// and "there is nothing left to send it to" are two different things to tell a
    /// listener.
    Unsupported,
}

/// What the command line asked this launch to connect to.
///
/// Asked for by the backend and carried out by the frontend: the window is the one place
/// a connection can ask its questions, such as a host-key dialog, and there is none until
/// the frontend is running.
///
/// A name nothing is saved under is a sentence rather than a silence: a windowed binary
/// has no console, so the window opens unconnected and says what was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "request")]
pub enum LaunchRequest {
    /// Start this saved connection, exactly as choosing its row in the Connect dialog would.
    ///
    /// The name as the document spells it, not as the switch did: the frontend looks the
    /// row up by name, and a lookup that had to allow for case would be a second place
    /// deciding what two names being the same means.
    Connect { name: String },
    /// Nothing is saved under this name, and [`said`](Self::Unknown::said) is what the
    /// unconnected window announces instead.
    Unknown { name: String, said: String },
}

impl LaunchRequest {
    /// The request for a name nothing is saved under, with the sentence already written,
    /// so the words a listener hears are one string in one place.
    pub fn unknown(name: &str) -> Self {
        Self::Unknown {
            name: name.to_owned(),
            said: no_such_connection(name),
        }
    }
}

/// What a listener is told about a name nothing is saved under, wherever it is met: on the
/// command line, or in a rename or a forget aimed at a row that is not there any more.
pub fn no_such_connection(name: &str) -> String {
    format!("There is no saved connection named {name}.")
}

/// The saved connections as the Connect dialog meets them.
///
/// Not [`StoredConnections`](crate::StoredConnections): that is what the document holds;
/// this is what that becomes once the machine has been asked — every row carrying the
/// profile its panel is loaded from, and whether this machine can start it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SavedConnections {
    /// The names, alphabetically and without case. Stable, never most-recent-first: a
    /// listener learns positions, and a list that reorders itself under them is a list
    /// they have to read from the top every time.
    pub rows: Vec<SavedRow>,
    /// What went wrong with a document that would not parse, and `None` when nothing
    /// did. The dialog says this where it would otherwise say the list is empty.
    pub unreadable: Option<String>,
}

/// One saved connection, as a row in that list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SavedRow {
    /// What the user called it, as they typed it — which is the whole of what the list
    /// shows, because a name is what they chose to recognise it by.
    pub name: String,
    /// What to load the panel from, and what to hand
    /// [`ConnectApi::use_profile`](crate::ConnectApi) if nothing in the panel is changed.
    ///
    /// Resolved against discovery rather than taken from the document: a saved
    /// PowerShell edition is matched to wherever it lives now, so an upgrade does not
    /// break a connection somebody saved a year ago.
    pub id: ProfileId,
    /// The kind and what identifies it, as one line a listener hears on arrowing onto the
    /// name: "SSH, marlon at example.org", "WSL, Ubuntu", "PowerShell 7".
    pub summary: String,
    /// Whether Acter may set this session up, so the panel's checkbox opens on what was
    /// saved.
    pub set_up: SetUp,
    /// Who holds the line when it opens, applied where the frontend already decides that
    /// — a saved choice wins over the default there.
    pub line_owner: LineOwner,
    /// Whether this machine can start it now. A distribution that was uninstalled, an
    /// edition that is gone and a scripted scenario in a release build are all listed and
    /// all unavailable, for the reason a missing kind is listed.
    pub available: bool,
    /// What to do about a row that cannot be started, and `None` when it can — a panel of
    /// instructions under a working row is noise a listener has to arrow past.
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
