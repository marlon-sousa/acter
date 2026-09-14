//! Entity/value: one saved connection — a name, what it connects to, and the two settings
//! a session has. Plus the rule a name has to keep, which is a rule about being read aloud.
//!
//! What it holds is the kind's own facts and never a resolved file path. A saved
//! PowerShell connection remembers the edition and the provenance the list showed it
//! under; the file is found again at connect time by matching those against what
//! discovery answers now, so upgrading PowerShell does not break a connection somebody
//! saved a year ago. A saved WSL connection remembers the distribution's name as
//! `wsl.exe -l -q` spelled it, and a saved SSH connection remembers the host, the port and
//! the account.
//!
//! A password is never in one, and neither is a passphrase. Both are asked at connect
//! time, in the window, and there is no field here that could hold either — the guarantee
//! is the type's rather than a reviewer's.
//!
//! Two settings and no more: whether Acter may set the session up, and who holds the line
//! when it opens. A starting directory and an auto-read threshold have a place in the
//! format and no implementation, because settings nothing reads are settings nothing tests.

use serde::{Deserialize, Serialize};

use crate::{ConnectionKind, LineOwner, ProfileId, SetUp};

/// The characters a name cannot contain, spelled out in words where a listener meets them
/// (see [`refused`]). Every name here is read aloud, and a name full of punctuation is
/// read aloud badly.
const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// What a listener is told when a name breaks the rule. Spelled out in words, because a
/// screen reader reads the characters themselves unreliably — a sentence containing a
/// bare `*` and a bare `?` is a sentence most readers render as something between silence
/// and noise.
const ILLEGAL: &str = "A name cannot contain slash, backslash, colon, star, question mark, \
                       quote, less than, greater than or bar.";

/// And what they are told when there is no name at all, which the rule also forbids and
/// which is the likelier mistake: a field left as it was found, or cleared and not refilled.
const EMPTY: &str = "A connection needs a name.";

/// What is said about a WSL session that named no distribution, which is the one profile
/// here that cannot be written down.
///
/// Saving it would write down "whatever WSL calls the default", and starting *that* again
/// is what New connection already does — so the row would answer nothing, and would quietly
/// answer something different the day somebody changed their default.
///
/// No window can reach this: the panel's Connect stays disabled until a distribution is
/// chosen. What is left is a value [`ProfileId`] admits and nothing constructs, and a
/// total function has to answer something, so it answers this rather than writing
/// nonsense down, and says what to do instead.
const NO_DISTRIBUTION: &str = "This session did not name a WSL distribution, so saving it \
                               would not give you anything to start again. Choose a \
                               distribution in New connection and save that.";

/// One connection somebody saved: what to call it, what it reaches, and how it opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    /// What the user called it, as they typed it. Kept as typed and compared without
    /// case, so a listener hears their own spelling back and two rows cannot differ by
    /// something nobody can hear.
    pub name: String,
    /// What it connects to.
    pub target: SavedTarget,
    /// Whether Acter may set the session up so it can say how commands went. Saved
    /// rather than asked again.
    #[serde(default = "set_up_by_default")]
    pub set_up: SetUp,
    /// Who holds the line when this connection opens. Saved so a connection that always
    /// wants Acter's line does not have to be told so every time.
    #[serde(default = "line_owner_by_default")]
    pub line_owner: LineOwner,
}

/// The default a document written before this field existed reads as, and the default the
/// Connect dialog's checkbox carries: ticked.
fn set_up_by_default() -> SetUp {
    SetUp::Yes
}

/// Who holds the line when nothing says: the far end, which is what a new session does.
/// Not [`LineOwner`]'s own `Default`, which is `Local` because that is the state a
/// session's *machinery* starts in; what this answers is what a connection somebody
/// saved before the field existed should open on, and that is the product's default
/// rather than the type's.
fn line_owner_by_default() -> LineOwner {
    LineOwner::FarEnd
}

/// What a saved connection reaches — one variant per kind, carrying only that kind's facts.
///
/// Tagged, so a kind added later is a new variant rather than a field somebody has to
/// remember to read. A document written by an older Acter still loads, because every
/// field a variant grows carries a serde default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target")]
pub enum SavedTarget {
    /// `cmd.exe`, which is one thing and has no facts of its own.
    Cmd,
    /// One PowerShell edition, and where the list found it.
    ///
    /// Not the resolved file path: the path is resolved again at connect time by
    /// matching these two against what discovery answers now, so a PowerShell upgrade
    /// does not break a saved connection. If nothing matches, the connection is listed
    /// and not available, with the same instructions the variant would carry.
    PowerShell {
        /// Which edition, as [`ConnectionKind`] names them.
        edition: ConnectionKind,
        /// What told this install from another of the same edition when it was saved:
        /// `preview`, `Microsoft Store`, or the directory it lives in. `None` on the
        /// ordinary machine with one install.
        #[serde(default)]
        provenance: Option<String>,
    },
    /// Bash inside one named WSL distribution.
    ///
    /// The name is required, and that is the whole point of saving one: a saved WSL
    /// connection earns its place by meaning "this distribution, no questions"; one that
    /// named none would be a row that opens whatever WSL calls the default, which is
    /// what New connection already does in one more keystroke — and which would
    /// silently become a different machine the day somebody changed their default.
    Wsl { distribution: String },
    /// A program named directly. No arguments field until something starts a program with
    /// arguments.
    Program { program: String },
    /// A machine that is not this one. Never a password and never a passphrase.
    Ssh {
        host: String,
        port: u16,
        account: String,
    },
    /// The shell this macOS account logs in to. Which shell that is is read from the
    /// machine at connect time, so it has nothing of its own to store.
    Terminal,
    /// One of the scripted far ends.
    ///
    /// Saveable in a debug build like any other kind, because the fake is a permanent
    /// supported session kind and because the end-to-end suite needs a saved connection
    /// it can start without a shell. A release build lists it as not available, since it
    /// never constructs one.
    Scripted { scenario: String },
}

impl SavedTarget {
    /// What this profile is, written down — or the sentence to say when it cannot be.
    ///
    /// The resolved file is deliberately dropped: an `Install` carries the file the list
    /// found; what survives here is the edition and the provenance, which is what can be
    /// matched against a machine that has changed since.
    ///
    /// One session cannot be written down at all, and it is the one that names no WSL
    /// distribution. See [`NO_DISTRIBUTION`].
    pub fn of(id: &ProfileId) -> Result<Self, String> {
        Ok(match id {
            ProfileId::Shell {
                kind: ConnectionKind::Cmd,
            }
            | ProfileId::Install {
                kind: ConnectionKind::Cmd,
                ..
            } => Self::Cmd,
            ProfileId::Shell {
                kind: ConnectionKind::Terminal,
            }
            | ProfileId::Install {
                kind: ConnectionKind::Terminal,
                ..
            } => Self::Terminal,
            ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            } => return Err(NO_DISTRIBUTION.to_owned()),
            ProfileId::Shell { kind } => Self::PowerShell {
                edition: *kind,
                provenance: None,
            },
            ProfileId::Install {
                kind, provenance, ..
            } => Self::PowerShell {
                edition: *kind,
                provenance: provenance.clone(),
            },
            ProfileId::Distribution { name } => Self::Wsl {
                distribution: name.clone(),
            },
            ProfileId::Program { program } => Self::Program {
                program: program.clone(),
            },
            ProfileId::Ssh { host, port, user } => Self::Ssh {
                host: host.clone(),
                port: *port,
                account: user.clone(),
            },
            ProfileId::Scripted { name } => Self::Scripted {
                scenario: name.clone(),
            },
        })
    }

    /// The profile this target names, before any machine has been asked about it.
    ///
    /// It is the fallback rather than the answer: what the Connect dialog's panel is
    /// loaded from is resolved against discovery, because a PowerShell edition can have
    /// moved and a distribution can be gone; this is what that resolution starts from and
    /// what a kind with nothing to resolve simply is.
    pub fn profile(&self) -> ProfileId {
        match self {
            Self::Cmd => ProfileId::Shell {
                kind: ConnectionKind::Cmd,
            },
            Self::Terminal => ProfileId::Shell {
                kind: ConnectionKind::Terminal,
            },
            Self::PowerShell { edition, .. } => ProfileId::Shell { kind: *edition },
            Self::Wsl { distribution } => ProfileId::Distribution {
                name: distribution.clone(),
            },
            Self::Program { program } => ProfileId::Program {
                program: program.clone(),
            },
            Self::Ssh {
                host,
                port,
                account,
            } => ProfileId::Ssh {
                host: host.clone(),
                port: *port,
                user: account.clone(),
            },
            Self::Scripted { scenario } => ProfileId::Scripted {
                name: scenario.clone(),
            },
        }
    }

    /// The one line a listener hears when they arrow onto this connection's name: the kind,
    /// and what identifies it.
    ///
    /// Commas rather than the colon [`ProfileId::label`] uses: that label names a row in
    /// a list of *kinds*, where the colon separates a category from a member; this is
    /// said after the user's own name for the connection, as a description of it, and a
    /// reader speaks a comma as the pause it is.
    pub fn summary(&self) -> String {
        match self {
            Self::Cmd => ConnectionKind::Cmd.label().to_owned(),
            Self::Terminal => ConnectionKind::Terminal.label().to_owned(),
            Self::PowerShell {
                edition,
                provenance: None,
            } => edition.label().to_owned(),
            Self::PowerShell {
                edition,
                provenance: Some(which),
            } => format!("{}, {which}", edition.label()),
            Self::Wsl { distribution } => format!("WSL, {distribution}"),
            Self::Program { program } => format!("Program, {program}"),
            // The account before the machine, because that is the order it is decided in,
            // and the port only when it is not the one every SSH server uses.
            Self::Ssh {
                host,
                port,
                account,
            } => {
                if *port == DEFAULT_SSH_PORT {
                    format!("SSH, {account} at {host}")
                } else {
                    format!("SSH, {account} at {host}, port {port}")
                }
            }
            Self::Scripted { scenario } => format!("Scripted, {scenario}"),
        }
    }
}

/// The port every SSH server listens on unless somebody moved it, which is the one a
/// listener does not need to hear on every row.
const DEFAULT_SSH_PORT: u16 = 22;

/// Why this name cannot be used, or `None` when it can.
///
/// A whole sentence rather than a code, for the reason every refusal in this domain
/// carries its own words: the sentence is what a listener hears, and it is decided in one
/// place rather than by whichever caller happened to meet the refusal.
pub fn refused(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some(EMPTY);
    }
    if name.contains(FORBIDDEN) {
        return Some(ILLEGAL);
    }
    None
}

/// Whether these two names are the same name.
///
/// Case-insensitively, and not for Windows' sake: two connections whose names differ
/// only in case are two rows a listener cannot tell apart, which is a reason that holds
/// on every platform this product will ever run on.
pub fn same_name(one: &str, another: &str) -> bool {
    one.trim().to_lowercase() == another.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh(port: u16) -> SavedTarget {
        SavedTarget::Ssh {
            host: "example.org".to_owned(),
            port,
            account: "marlon".to_owned(),
        }
    }

    #[test]
    fn every_kind_survives_being_written_down_and_read_back() {
        let cases = [
            ProfileId::Shell {
                kind: ConnectionKind::Cmd,
            },
            ProfileId::Shell {
                kind: ConnectionKind::Terminal,
            },
            ProfileId::Shell {
                kind: ConnectionKind::PowerShellSeven,
            },
            ProfileId::Distribution {
                name: "Ubuntu 24.04".to_owned(),
            },
            ProfileId::Program {
                program: "nu.exe".to_owned(),
            },
            ProfileId::Ssh {
                host: "example.org".to_owned(),
                port: 2222,
                user: "marlon".to_owned(),
            },
            ProfileId::Scripted {
                name: "builtin".to_owned(),
            },
        ];

        for id in cases {
            let target = SavedTarget::of(&id).expect("every one of these can be saved");
            let json = serde_json::to_value(&target).expect("a target is written down");
            let back: SavedTarget = serde_json::from_value(json).expect("and read back");

            assert_eq!(back, target, "{id:?}");
            assert_eq!(target.profile(), id, "{id:?} names itself again");
        }
    }

    #[test]
    fn a_wsl_session_with_no_distribution_cannot_be_saved_and_says_what_to_do() {
        let refused = SavedTarget::of(&ProfileId::Shell {
            kind: ConnectionKind::Wsl,
        })
        .expect_err("there is nothing here to start again");

        assert_eq!(refused, NO_DISTRIBUTION);
        assert!(refused.contains("New connection"), "{refused}");
        assert!(refused.ends_with('.'), "it is read aloud: {refused}");
        assert!(!refused.contains("  "), "with no run of spaces: {refused}");
    }

    #[test]
    fn a_named_distribution_is_what_is_written_down_and_what_comes_back() {
        let saved = SavedTarget::of(&ProfileId::Distribution {
            name: "Ubuntu 24.04".to_owned(),
        })
        .expect("a named distribution is saveable");

        assert_eq!(
            saved,
            SavedTarget::Wsl {
                distribution: "Ubuntu 24.04".to_owned()
            }
        );
        assert_eq!(
            saved.profile(),
            ProfileId::Distribution {
                name: "Ubuntu 24.04".to_owned()
            }
        );
        assert_eq!(saved.summary(), "WSL, Ubuntu 24.04");
    }

    #[test]
    fn a_powershell_install_keeps_its_edition_and_forgets_its_path() {
        let saved = SavedTarget::of(&ProfileId::Install {
            kind: ConnectionKind::PowerShellSeven,
            program: r"C:\Program Files\PowerShell\7\pwsh.exe".to_owned(),
            provenance: Some("preview".to_owned()),
        })
        .expect("an install is saveable");

        assert_eq!(
            saved,
            SavedTarget::PowerShell {
                edition: ConnectionKind::PowerShellSeven,
                provenance: Some("preview".to_owned()),
            }
        );
        let written = serde_json::to_string(&saved).expect("it is written down");
        assert!(
            !written.contains("pwsh.exe"),
            "a resolved path in the document is the thing decision 7 removes: {written}"
        );
    }

    #[test]
    fn a_terminal_connection_stores_no_shell_because_the_machine_answers_that() {
        let saved = SavedTarget::of(&ProfileId::Install {
            kind: ConnectionKind::Terminal,
            program: "/bin/zsh".to_owned(),
            provenance: Some("zsh".to_owned()),
        })
        .expect("a Terminal shell is saveable");

        assert_eq!(saved, SavedTarget::Terminal);
        assert_eq!(
            saved.profile(),
            ProfileId::Shell {
                kind: ConnectionKind::Terminal
            }
        );
    }

    #[test]
    fn arrowing_onto_a_name_describes_the_kind_and_what_identifies_it() {
        assert_eq!(ssh(22).summary(), "SSH, marlon at example.org");
        assert_eq!(
            ssh(2222).summary(),
            "SSH, marlon at example.org, port 2222",
            "a port nobody would assume is said"
        );
        assert_eq!(
            SavedTarget::Wsl {
                distribution: "Ubuntu".to_owned()
            }
            .summary(),
            "WSL, Ubuntu"
        );
        assert_eq!(
            SavedTarget::PowerShell {
                edition: ConnectionKind::PowerShellSeven,
                provenance: None
            }
            .summary(),
            "PowerShell 7"
        );
        assert_eq!(SavedTarget::Cmd.summary(), "Command Prompt");
    }

    #[test]
    fn every_summary_is_a_phrase_a_reader_can_speak() {
        let targets = [
            SavedTarget::Cmd,
            SavedTarget::Terminal,
            SavedTarget::PowerShell {
                edition: ConnectionKind::WindowsPowerShell,
                provenance: Some("Microsoft Store".to_owned()),
            },
            SavedTarget::Wsl {
                distribution: "Debian".to_owned(),
            },
            SavedTarget::Program {
                program: "nu.exe".to_owned(),
            },
            ssh(22),
            SavedTarget::Scripted {
                scenario: "builtin".to_owned(),
            },
        ];

        for target in targets {
            let said = target.summary();
            assert!(!said.trim().is_empty(), "{target:?} says nothing");
            assert!(
                !said.contains("  "),
                "a spoken phrase carries no run of spaces: {said}"
            );
            assert!(
                !said.contains(':'),
                "a colon is the list's punctuation, not a description's: {said}"
            );
        }
    }

    #[test]
    fn a_name_full_of_punctuation_is_refused_in_words() {
        for bad in ['/', '\\', ':', '*', '?', '"', '<', '>', '|'] {
            let name = format!("work{bad}laptop");
            assert_eq!(refused(&name), Some(ILLEGAL), "{name} should be refused");
        }
        assert!(
            !ILLEGAL.contains('*') && !ILLEGAL.contains('?') && !ILLEGAL.contains('\\'),
            "the sentence names the characters in words rather than printing them: {ILLEGAL}"
        );
        assert!(!ILLEGAL.contains("  "), "and carries no run of spaces");
    }

    #[test]
    fn a_name_that_is_only_spaces_is_no_name() {
        assert_eq!(refused(""), Some(EMPTY));
        assert_eq!(refused("   "), Some(EMPTY));
        assert_eq!(refused("work laptop"), None, "an ordinary name is fine");
        assert_eq!(
            refused("Ubuntu 24.04 (work)"),
            None,
            "brackets, dots and spaces are all things a reader says"
        );
    }

    #[test]
    fn names_are_compared_without_case_and_without_stray_spaces() {
        assert!(same_name("Work Laptop", "work laptop"));
        assert!(same_name(" work laptop ", "work laptop"));
        assert!(!same_name("work laptop", "work desktop"));
    }

    #[test]
    fn a_connection_written_by_an_older_acter_loads_with_the_ordinary_defaults() {
        let older = serde_json::json!({
            "name": "work laptop",
            "target": { "target": "Cmd" }
        });

        let read: SavedConnection = serde_json::from_value(older).expect("it still loads");

        assert_eq!(read.set_up, SetUp::Yes, "the checkbox's own default");
        assert_eq!(
            read.line_owner,
            LineOwner::FarEnd,
            "and the line goes where a new session's line goes"
        );
    }
}
