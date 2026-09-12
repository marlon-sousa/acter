//! Entity/value: one saved connection — a name, what it connects to, and the two settings
//! a session has. Plus the rule a name has to keep, which is a rule about being read aloud.
//!
//! **What it holds is the kind's own facts and never a resolved file path** (spec 26,
//! decision 7). A saved PowerShell connection remembers the edition and the provenance the
//! list showed it under; the file is found again at connect time by matching those against
//! what discovery answers now, so upgrading PowerShell does not break a connection somebody
//! saved a year ago. A saved WSL connection remembers the distribution's name as
//! `wsl.exe -l -q` spelled it, and a saved SSH connection remembers the host, the port and
//! the account.
//!
//! **A password is never in one, and neither is a passphrase.** Both are asked at connect
//! time, in the window, and there is no field here that could hold either — the guarantee is
//! the type's rather than a reviewer's (spec B9, decision 5).
//!
//! **Two settings and no more**: whether Acter may set the session up, and who holds the
//! line when it opens. A starting directory and an auto-read threshold have a place in the
//! format and no implementation, because settings nothing reads are settings nothing tests.

use serde::{Deserialize, Serialize};

use crate::{ConnectionKind, LineOwner, ProfileId, SetUp};

/// The characters a name cannot contain, spelled out in words where a listener meets them
/// (see [`refused`]).
///
/// **The reason changed and the rule did not** (spec 26, decision 8). It used to be a
/// filesystem rule, because a name was a file name; now every name here is read aloud, and
/// a name full of punctuation is read aloud badly.
const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// What a listener is told when a name breaks the rule.
///
/// **Spelled out in words, because a screen reader reads the characters themselves
/// unreliably** — a sentence containing a bare `*` and a bare `?` is a sentence most readers
/// render as something between silence and noise.
const ILLEGAL: &str = "A name cannot contain slash, backslash, colon, star, question mark, \
                       quote, less than, greater than or bar.";

/// And what they are told when there is no name at all, which the rule also forbids and
/// which is the likelier mistake: a field left as it was found, or cleared and not refilled.
const EMPTY: &str = "A connection needs a name.";

/// One connection somebody saved: what to call it, what it reaches, and how it opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    /// What the user called it, as they typed it. **Kept as typed and compared without
    /// case**, so a listener hears their own spelling back and two rows cannot differ by
    /// something nobody can hear (decision 8).
    pub name: String,
    /// What it connects to.
    pub target: SavedTarget,
    /// Whether Acter may set the session up so it can say how commands went (spec B9.5,
    /// decision 9). Saved rather than asked again, which is what B9.5's decision 10 parked
    /// until there was a connection to keep it in.
    #[serde(default = "set_up_by_default")]
    pub set_up: SetUp,
    /// Who holds the line when this connection opens (spec 28, decision 1). Saved for
    /// B9.5's reason one entry later, and what closes roadmap 28.8: a connection that
    /// always wants Acter's line no longer has to be told so every time.
    #[serde(default = "line_owner_by_default")]
    pub line_owner: LineOwner,
}

/// The default a document written before this field existed reads as, and the default the
/// Connect dialog's checkbox carries: ticked.
fn set_up_by_default() -> SetUp {
    SetUp::Yes
}

/// And who holds the line when nothing says: the far end, which is what a new session does
/// (roadmap 28.7). **Not [`LineOwner`]'s own `Default`**, which is `Local` because that is
/// the state a session's *machinery* starts in; what this answers is what a connection
/// somebody saved before the field existed should open on, and that is the product's
/// default rather than the type's.
fn line_owner_by_default() -> LineOwner {
    LineOwner::FarEnd
}

/// What a saved connection reaches — one variant per kind, carrying only that kind's facts.
///
/// **Tagged, so a kind added later is a new variant rather than a field somebody has to
/// remember to read.** A document written by an older Acter still loads, because every
/// field a variant grows carries a serde default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target")]
pub enum SavedTarget {
    /// `cmd.exe`, which is one thing and has no facts of its own.
    Cmd,
    /// One PowerShell edition, and where the list found it.
    ///
    /// **Not the resolved file path** (decision 7). The path is resolved again at connect
    /// time by matching these two against what discovery answers now, so a PowerShell
    /// upgrade does not break a saved connection. If nothing matches, the connection is
    /// listed and not available, with the same instructions the variant would carry.
    PowerShell {
        /// Which edition, as [`ConnectionKind`] names them.
        edition: ConnectionKind,
        /// What told this install from another of the same edition when it was saved:
        /// `preview`, `Microsoft Store`, or the directory it lives in. `None` on the
        /// ordinary machine with one install.
        #[serde(default)]
        provenance: Option<String>,
    },
    /// Bash inside one WSL distribution.
    Wsl {
        /// The distribution's name, as `wsl.exe -l -q` spelled it — or `None` for whatever
        /// distribution WSL calls the default, which is what a session started from
        /// `ACTER_SHELL=wsl` is and what saving one has to be able to write down.
        #[serde(default)]
        distribution: Option<String>,
    },
    /// A program named directly. No arguments field until something starts a program with
    /// arguments.
    Program { program: String },
    /// A machine that is not this one.
    ///
    /// **Never a password and never a passphrase** (spec B9, decision 5). A key file path
    /// joins this when B9.1 lands, not before.
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
    /// **Saveable in a debug build like any other kind**, because the fake is a permanent
    /// supported session kind (DESIGN) and because the end-to-end suite needs a saved
    /// connection it can start without a shell. A release build lists it as not available,
    /// since it never constructs one (spec B7, decision 7).
    Scripted { scenario: String },
}

impl SavedTarget {
    /// What this profile is, written down.
    ///
    /// **The resolved file is deliberately dropped** (decision 7). An `Install` carries the
    /// file the list found; what survives here is the edition and the provenance, which is
    /// what can be matched against a machine that has changed since.
    pub fn of(id: &ProfileId) -> Self {
        match id {
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
            } => Self::Wsl { distribution: None },
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
                distribution: Some(name.clone()),
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
        }
    }

    /// The profile this target names, before any machine has been asked about it.
    ///
    /// **It is the fallback rather than the answer.** What the Connect dialog's panel is
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
            Self::Wsl { distribution: None } => ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            },
            Self::Wsl {
                distribution: Some(name),
            } => ProfileId::Distribution { name: name.clone() },
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
    /// and what identifies it (spec 26, decision 13).
    ///
    /// **Commas rather than the colon [`ProfileId::label`] uses.** That label names a row
    /// in a list of *kinds*, where the colon separates a category from a member; this is
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
            Self::Wsl {
                distribution: Some(name),
            } => format!("WSL, {name}"),
            // It names no distribution because nothing here chose one: asking WSL for its
            // default is deliberately not the same as this program deciding which one that
            // is (spec B5.3).
            Self::Wsl { distribution: None } => "WSL, the default distribution".to_owned(),
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

/// Why this name cannot be used, or `None` when it can (spec 26, decision 8).
///
/// **A whole sentence rather than a code**, for the reason every refusal in this domain
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
/// **Case-insensitively, and not for Windows' sake** (decision 8). Two connections whose
/// names differ only in case are two rows a listener cannot tell apart, which is a reason
/// that holds on every platform this product will ever run on.
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

    /// **Every kind round-trips through what is written down** (definition of done 3): a
    /// profile becomes a target, and the target names a profile the dialog can load a panel
    /// from. What is deliberately *not* preserved is the resolved file, which is decision 7.
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
                kind: ConnectionKind::Wsl,
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
            let target = SavedTarget::of(&id);
            let json = serde_json::to_value(&target).expect("a target is written down");
            let back: SavedTarget = serde_json::from_value(json).expect("and read back");

            assert_eq!(back, target, "{id:?}");
            assert_eq!(target.profile(), id, "{id:?} names itself again");
        }
    }

    /// **The file is dropped and the edition is kept**, which is what makes a saved
    /// PowerShell connection survive its edition moving to a different path (definition of
    /// done 5). What comes back names the edition, and the machine is asked where it is now.
    #[test]
    fn a_powershell_install_keeps_its_edition_and_forgets_its_path() {
        let saved = SavedTarget::of(&ProfileId::Install {
            kind: ConnectionKind::PowerShellSeven,
            program: r"C:\Program Files\PowerShell\7\pwsh.exe".to_owned(),
            provenance: Some("preview".to_owned()),
        });

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

    /// A Terminal row's id carries the shell `/etc/shells` named, and that file is read
    /// again at connect time — so what is written down is the kind and nothing else.
    #[test]
    fn a_terminal_connection_stores_no_shell_because_the_machine_answers_that() {
        let saved = SavedTarget::of(&ProfileId::Install {
            kind: ConnectionKind::Terminal,
            program: "/bin/zsh".to_owned(),
            provenance: Some("zsh".to_owned()),
        });

        assert_eq!(saved, SavedTarget::Terminal);
        assert_eq!(
            saved.profile(),
            ProfileId::Shell {
                kind: ConnectionKind::Terminal
            }
        );
    }

    /// **The one line a listener hears on arrowing onto a name** (decision 13): the kind,
    /// and what identifies it. Asserted as whole strings because that is the utterance.
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
                distribution: Some("Ubuntu".to_owned())
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

    /// Every summary is something a listener can hear: no empty string, no punctuation
    /// standing in for a word, and no run of spaces from a line continuation a formatting
    /// pass unwrapped.
    #[test]
    fn every_summary_is_a_phrase_a_reader_can_speak() {
        let targets = [
            SavedTarget::Cmd,
            SavedTarget::Terminal,
            SavedTarget::PowerShell {
                edition: ConnectionKind::WindowsPowerShell,
                provenance: Some("Microsoft Store".to_owned()),
            },
            SavedTarget::Wsl { distribution: None },
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

    /// **The name rule, as one sentence** (decision 8). Every forbidden character is
    /// refused, and the sentence spells them out rather than printing them.
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

    /// A name that is nothing at all is refused too, and with a different sentence: it is
    /// the likelier mistake, and "cannot contain slash" answers a question nobody asked.
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

    /// **Two names that differ only in case are one name** (decision 8): two rows a
    /// listener cannot tell apart is the thing this prevents.
    #[test]
    fn names_are_compared_without_case_and_without_stray_spaces() {
        assert!(same_name("Work Laptop", "work laptop"));
        assert!(same_name(" work laptop ", "work laptop"));
        assert!(!same_name("work laptop", "work desktop"));
    }

    /// A document written before a field existed still loads, which is what the serde
    /// defaults are for (decision 2): the two settings take the values a new connection
    /// would have.
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
