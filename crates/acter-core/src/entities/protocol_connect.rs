//! Entity/value: what connecting says while it happens, what it asks, and what it is told
//! back.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Connected;

/// Minted per attempt, so an answer to an abandoned dialog never resolves a newer attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AttemptId(pub u32);

/// One thing that happens while a connection is being made; `Arrived` or `Failed` ends the
/// sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "step")]
pub enum ConnectStep {
    /// A complete sentence, spoken as it arrives.
    Progress {
        said: String,
    },
    Asked {
        attempt: AttemptId,
        question: ConnectQuestion,
    },
    Arrived {
        connected: Connected,
    },
    /// One sentence a listener can act on.
    Failed {
        why: String,
    },
}

/// What a person is asked, mid-connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "question")]
pub enum ConnectQuestion {
    HostKey {
        host: String,
        port: u16,
        /// As `ssh-keygen -l` prints it.
        fingerprint: String,
        /// The fingerprint on file, and `None` for a host with no record.
        recorded: Option<String>,
        /// Something true that is not the answer, such as an unreadable `known_hosts`, and
        /// `None` when there is nothing.
        aside: Option<String>,
    },
    /// The file this machine is about to start did not verify.
    Unverified {
        label: String,
        /// The full path.
        program: String,
        /// Whole sentences ending in what to do next.
        said: String,
        /// Who signed it, and `None` when nothing could be read.
        signer: Option<String>,
    },
    /// What Acter would run inside this shell to integrate it.
    SetUpSession {
        shell: String,
        detected: String,
        offer: String,
        command: String,
        refusal: String,
    },
    Password {
        host: String,
        user: String,
        /// Whether a password was already tried and refused.
        again: bool,
    },
}

/// What the person decided; never serialized, since [`Secret`](crate::Secret) cannot be.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "answer")]
pub enum ConnectAnswer {
    /// Trust this server and record it.
    Trust,
    Password {
        secret: crate::Secret,
    },
    /// The only way to reach [`ProgramAnswer::Start`](crate::ProgramAnswer).
    StartAnyway,
    /// The only way to reach [`SetupAnswer::SetUp`](crate::SetupAnswer); `remember` is kept
    /// per shell, not per host or profile.
    SetUpSession {
        remember: bool,
    },
    /// Refused, cancelled, or closed.
    GiveUp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineOwner, Secret, SessionId};
    use serde_json::json;

    #[test]
    fn every_step_round_trips_with_the_tag_the_frontend_switches_on() {
        let steps = [
            ConnectStep::Progress {
                said: "Connecting to acter-ssh.".to_owned(),
            },
            ConnectStep::Asked {
                attempt: AttemptId(1),
                question: ConnectQuestion::Password {
                    host: "acter-ssh".to_owned(),
                    user: "acter".to_owned(),
                    again: true,
                },
            },
            ConnectStep::Asked {
                attempt: AttemptId(2),
                question: ConnectQuestion::Unverified {
                    label: "PowerShell 7".to_owned(),
                    program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
                    said: "Nothing has signed this file.".to_owned(),
                    signer: None,
                },
            },
            ConnectStep::Arrived {
                connected: Connected {
                    session: SessionId(2),
                    label: "SSH: acter at acter-ssh".to_owned(),
                    note: None,
                    limit_explained: false,
                    saved_as: None,
                    line_owner: LineOwner::FarEnd,
                },
            },
            ConnectStep::Failed {
                why: "Acter could not reach acter-ssh on port 22.".to_owned(),
            },
        ];

        for step in steps {
            let json = serde_json::to_value(&step).expect("a step serializes");
            assert!(json.get("step").is_some(), "every step is tagged: {json}");
            assert_eq!(
                serde_json::from_value::<ConnectStep>(json).expect("and comes back"),
                step
            );
        }
    }

    #[test]
    fn a_changed_key_carries_what_was_on_file_and_an_unknown_one_does_not() {
        let unknown = ConnectQuestion::HostKey {
            host: "acter-ssh".to_owned(),
            port: 2222,
            fingerprint: "SHA256:new".to_owned(),
            recorded: None,
            aside: None,
        };
        let changed = ConnectQuestion::HostKey {
            host: "acter-ssh".to_owned(),
            port: 2222,
            fingerprint: "SHA256:new".to_owned(),
            recorded: Some("SHA256:old".to_owned()),
            aside: None,
        };

        assert_ne!(unknown, changed);
    }

    #[test]
    fn an_answer_arrives_from_the_wire() {
        assert_eq!(
            serde_json::from_value::<ConnectAnswer>(json!({ "answer": "Trust" }))
                .expect("a decision arrives"),
            ConnectAnswer::Trust
        );
        assert_eq!(
            serde_json::from_value::<ConnectAnswer>(json!({ "answer": "StartAnyway" }))
                .expect("so does a decision about a file"),
            ConnectAnswer::StartAnyway
        );

        let answered = serde_json::from_value::<ConnectAnswer>(
            json!({ "answer": "Password", "secret": "hunter2" }),
        )
        .expect("a password arrives");

        assert_eq!(
            answered,
            ConnectAnswer::Password {
                secret: Secret::new("hunter2")
            }
        );
    }

    #[test]
    fn a_password_that_arrived_cannot_be_printed_on_the_way_past() {
        let answered = ConnectAnswer::Password {
            secret: Secret::new("hunter2"),
        };

        let printed = format!("{answered:?}");

        assert!(
            !printed.contains("hunter2"),
            "debugging an answer must not print the password: {printed}"
        );
    }
}
