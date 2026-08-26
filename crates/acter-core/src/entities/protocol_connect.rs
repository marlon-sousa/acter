//! Entity/value: what connecting says while it happens, what it asks, and what it is told
//! back.
//!
//! **Connecting stopped being one call and answer with B9.** Every far end before SSH could
//! be started with a single request: it worked, or it failed with one speakable sentence.
//! An SSH connection stops partway on things that are not failures — a host key nobody has
//! seen, a password nobody has typed — and each is a question for the person in front of
//! the window, asked before there is a session to ask it in (spec B9).
//!
//! **Why this is a stream out and separate invokes back, rather than one invoke that
//! waits.** Tauri has no way for a backend to ask the frontend something and await the
//! answer: events and channels are one-way, and `eval` returns nothing. That leaves one
//! shape — a request out, an answer back as its own call — and one hard constraint on it:
//! a `#[tauri::command]` without `async` **runs on the main thread**, so an invoke that
//! blocked waiting for a dialog would be holding the very thread the answering invoke needs
//! in order to be dispatched. It would not be slow; it would deadlock, at the exact moment
//! the host-key dialog appeared. So every invoke here returns at once, the steps travel on
//! a `Channel` — the same mechanism `attach_session` already uses — and the waiting happens
//! on a task of the connection's own.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Connected;

/// Which attempt to connect a step or an answer belongs to.
///
/// **Minted per attempt, for the reason [`SessionId`](crate::SessionId) is minted per
/// connection** (spec B7, decision 4): a user who gives up on one dialog and starts again
/// has two conversations in flight for a moment, and an answer typed into the first must
/// never resolve the second. A password is the worst possible value to deliver to the wrong
/// question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AttemptId(pub u32);

/// One thing that happens while a connection is being made.
///
/// The frontend reads these in order until one of the last two arrives, which is what ends
/// the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "step")]
pub enum ConnectStep {
    /// Something is happening and it is worth saying out loud.
    ///
    /// **A listener with no feedback cannot tell a slow network from a dead one** (spec B9,
    /// decision 6), and an SSH connection can take seconds before anything at all is
    /// certain. The sentence is complete and is read exactly as it arrives.
    Progress { said: String },
    /// The connection cannot go on until somebody answers this.
    Asked {
        attempt: AttemptId,
        question: ConnectQuestion,
    },
    /// There is a session, and this is what to attach to.
    Arrived { connected: Connected },
    /// There is no session, and this is why — one sentence a listener can act on.
    Failed { why: String },
}

/// What a person is asked, mid-connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "question")]
pub enum ConnectQuestion {
    /// This server's identity is not one Acter has a record of.
    ///
    /// **An unknown key and a changed one are the same variant carrying different facts,
    /// deliberately**: they are one decision — trust this server or do not — and the
    /// dialog's words differ rather than its shape. What makes them different sentences is
    /// [`recorded`](Self::HostKey::recorded) being present.
    HostKey {
        host: String,
        port: u16,
        /// What this server offered, as `ssh-keygen -l` prints it, so it can be compared
        /// against what a provider or a colleague gave the user.
        fingerprint: String,
        /// The fingerprint that was on file, when one was — which is what makes this the
        /// serious question rather than the routine one. `None` for a host nobody has a
        /// record of.
        recorded: Option<String>,
        /// Something true that is not the answer: a `known_hosts` file that could not be
        /// read, so the user knows this may be being asked about a host they already trust.
        aside: Option<String>,
    },
    /// The server will take a password, and there is not one yet.
    Password {
        host: String,
        user: String,
        /// Whether one was already tried and refused.
        ///
        /// **Said rather than left to be inferred from the dialog opening twice**, which is
        /// indistinguishable from the first one not having been submitted.
        again: bool,
    },
}

/// What the person decided.
///
/// **Deserialize only.** An answer arrives from a dialog and never travels the other way,
/// and deriving `Serialize` here would not compile: [`Secret`](crate::Secret) has none. The
/// guarantee is the type system's rather than a reviewer's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "answer")]
pub enum ConnectAnswer {
    /// Trust this server, and remember it so the same host does not ask again.
    Trust,
    /// Here is the password.
    ///
    /// Carries a [`Secret`](crate::Secret), which deserializes from the wire and can never
    /// be serialized back onto it, printed, or logged.
    Password { secret: crate::Secret },
    /// Stop: the key was refused, the dialog was cancelled, or the user changed their mind.
    ///
    /// **One variant for all three**, because the connection does the same thing for each
    /// and the sentence a listener hears is about what did *not* happen rather than about
    /// which control they used to say so.
    GiveUp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Secret, SessionId};
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
            ConnectStep::Arrived {
                connected: Connected {
                    session: SessionId(2),
                    label: "SSH: acter at acter-ssh".to_owned(),
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

    /// The two questions are one shape with different facts, and the difference between the
    /// routine one and the serious one is a recorded fingerprint being there.
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

    /// An answer arrives from the frontend, so it has to deserialize — and the password one
    /// carries a value that can never travel the other way.
    #[test]
    fn an_answer_arrives_from_the_wire() {
        assert_eq!(
            serde_json::from_value::<ConnectAnswer>(json!({ "answer": "Trust" }))
                .expect("a decision arrives"),
            ConnectAnswer::Trust
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

    /// **The guarantee, asserted rather than commented.** A password can be read off the
    /// wire and can never be put back on it: `ConnectAnswer` deliberately derives no
    /// `Serialize`, so this is the one direction that compiles — and the debug tape, which
    /// records what crosses the invoke boundary, has nothing it could record.
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
