//! Entity/value: one saved connection — a name, what it connects to, and how it opens —
//! and the rule a name has to keep.

use serde::{Deserialize, Serialize};

use crate::{ConnectionKind, LineOwner, ProfileId, SetUp};

const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Names the characters in words, because screen readers speak a bare `*` or `?` unreliably.
const ILLEGAL: &str = "A name cannot contain slash, backslash, colon, star, question mark, \
                       quote, less than, greater than or bar.";

const EMPTY: &str = "A connection needs a name.";

/// Unreachable from the window, whose Connect stays disabled until a distribution is chosen.
const NO_DISTRIBUTION: &str = "This session did not name a WSL distribution, so saving it \
                               would not give you anything to start again. Choose a \
                               distribution in New connection and save that.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    /// Kept as typed; compared with [`same_name`].
    pub name: String,
    pub target: SavedTarget,
    #[serde(default = "set_up_by_default")]
    pub set_up: SetUp,
    #[serde(default = "line_owner_by_default")]
    pub line_owner: LineOwner,
}

fn set_up_by_default() -> SetUp {
    SetUp::Yes
}

/// A new session opens with the far end holding the line, unlike `LineOwner::default()`.
fn line_owner_by_default() -> LineOwner {
    LineOwner::FarEnd
}

/// Every field a variant gains needs a serde default, so documents written by an older Acter
/// still load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target")]
pub enum SavedTarget {
    Cmd,
    /// No file path: the install is matched again at connect time, so an upgrade does not
    /// break it, and no match lists the connection as not available.
    PowerShell {
        edition: ConnectionKind,
        #[serde(default)]
        provenance: Option<String>,
    },
    Wsl {
        distribution: String,
    },
    Program {
        program: String,
    },
    /// Never a password or a passphrase.
    Ssh {
        host: String,
        port: u16,
        account: String,
    },
    /// The macOS account's login shell, read at connect time.
    Terminal,
    /// Listed as not available in a release build.
    Scripted {
        scenario: String,
    },
}

impl SavedTarget {
    /// Rejects with a whole spoken sentence for WSL with no distribution, which would
    /// silently follow whatever WSL's default becomes.
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

    /// Not yet resolved against this machine's installs.
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

    /// Commas where [`ProfileId::label`] has a colon, since a screen reader pauses at a comma.
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

const DEFAULT_SSH_PORT: u16 = 22;

/// The spoken sentence saying why this name cannot be used, or `None` when it can.
pub fn refused(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some(EMPTY);
    }
    if name.contains(FORBIDDEN) {
        return Some(ILLEGAL);
    }
    None
}

/// Case-insensitive, since names differing only in case sound the same.
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
