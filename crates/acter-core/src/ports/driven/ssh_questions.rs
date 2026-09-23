//! Port (driven): the questions an SSH connection has to ask a person, in the window
//! where there is no session to ask them in.
//!
//! Every method blocks until somebody answers, so call it from a task of the connection's own,
//! never from a Tauri invoke, which runs on the main thread and would deadlock the answer.

pub trait SshQuestions: Send + Sync {
    /// An implementer with nobody to ask answers `Refuse`.
    fn host_key(&self, question: HostKeyQuestion) -> HostKeyAnswer;

    /// `None` is the user declining to give one, which ends the attempt.
    fn password(&self, question: PasswordQuestion) -> Option<Secret>;

    /// The sentence is read aloud exactly as it arrives, so it must be whole and speakable.
    fn tell(&self, sentence: &str);
}

pub struct Unasked;

impl SshQuestions for Unasked {
    fn host_key(&self, _question: HostKeyQuestion) -> HostKeyAnswer {
        HostKeyAnswer::Refuse
    }

    fn password(&self, _question: PasswordQuestion) -> Option<Secret> {
        None
    }

    fn tell(&self, _sentence: &str) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    /// In `ssh-keygen -l` form; see crates/acter-transports/src/ssh/known_hosts.rs.
    pub fingerprint: String,
    pub state: HostKeyState,
    /// A sentence to say beside the question, such as a `known_hosts` that could not be read;
    /// `None` when there is nothing to add.
    pub aside: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyState {
    Unknown,
    Changed { recorded: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyAnswer {
    Accept,
    Refuse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordQuestion {
    pub host: String,
    pub user: String,
    /// Whether the server already refused a password in this attempt.
    pub again: bool,
}

/// Never give this `Display` or `Serialize`, and never let `Debug` print the value, or a
/// password can reach a log or the debug event tape; it does not scrub memory.
#[derive(Clone, PartialEq, Eq, serde::Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(not shown)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_does_not_print_itself() {
        let secret = Secret::new("hunter2");

        let printed = format!("{secret:?}");

        assert!(
            !printed.contains("hunter2"),
            "a debug print must not carry the password: {printed}"
        );
        assert_eq!(printed, "Secret(not shown)");
    }

    #[test]
    fn the_one_caller_that_has_to_send_it_can_read_it() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
    }

    #[test]
    fn an_unknown_key_and_a_changed_key_are_different_states() {
        let changed = HostKeyState::Changed {
            recorded: "SHA256:something".to_owned(),
        };

        assert_ne!(HostKeyState::Unknown, changed);
    }
}
