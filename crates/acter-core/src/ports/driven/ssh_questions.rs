//! Port (driven): the questions an SSH connection has to ask a person, in the window
//! where there is no session to ask them in.
//!
//! **This is what makes SSH different from every transport before it.** `LocalPty`
//! spawns a process and the operating system does the rest; a failure is one speakable
//! sentence and there is nothing to decide. SSH has to *establish* something first, and
//! establishing it can stop on things that are not errors at all: a host key nobody has
//! seen before, a password nobody has typed yet. Each of those is a question the far end
//! asks the *user*, before there is a session (spec B9).
//!
//! **The domain never opens a dialog, and the transport never knows there is one.** The
//! adapter behind this port is free to be a modal dialog with a real accessible name, a
//! test's canned answer, or a rig harness that accepts everything — and the SSH transport
//! cannot tell which it is talking to. That is the whole reason the questions are a port
//! rather than a callback into the frontend: the transport is measured against a real
//! server with no window anywhere near it.
//!
//! **Why a library rather than `ssh.exe`, said in types.** `ssh.exe` writes "Are you sure
//! you want to continue connecting" and a password prompt into a terminal it owns, and
//! Acter would have to recognise localised, version-dependent English in a byte stream to
//! know a question had been asked at all. Here an unknown host key is
//! [`SshQuestions::host_key`] and a password is [`SshQuestions::password`], so Acter knows
//! what it is asking and what the answer was for (spec B9, decision 1).
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. Blocking is the point
//! of it: the connection genuinely cannot proceed until somebody answers, and the thing
//! that waits is a task of the transport's own — never an invoke, which Tauri runs on the
//! main thread and which would deadlock the answer it is waiting for.

/// The questions, and the one statement, that reach a person mid-connection.
///
/// `Send + Sync` because the connecting task owns one and calls it from wherever the SSH
/// client's handler runs; `&self` because answering a question changes nothing about the
/// asker.
pub trait SshQuestions: Send + Sync {
    /// This server offered a key that has not been seen before, or one that has *changed*.
    ///
    /// **Refusal is the safe answer and the implementer is expected to default to it**
    /// (spec B9, decision 3). Acter never silently trusts: there is no "accept everything"
    /// mode, and a connection whose key was refused reports that as its own speakable
    /// failure rather than retrying without asking.
    ///
    /// Never called for a key already recorded — a host the user has trusted before
    /// connects without being asked again, which is what stops a populated `known_hosts`
    /// turning into a sequence of prompts.
    fn host_key(&self, question: HostKeyQuestion) -> HostKeyAnswer;

    /// The server will accept a password, and there is none yet.
    ///
    /// `None` is the user declining to give one, which ends the attempt: it is a decision
    /// rather than a failure, and it is reported as one.
    ///
    /// **The value never touches the session's edit field, the terminal buffer, the
    /// transcript recorder or any log** (spec B9, decision 4). See [`Secret`], which has no
    /// `Display` and a `Debug` that says nothing, so the ordinary ways a value leaks into a
    /// diagnostic are closed rather than merely unused.
    fn password(&self, question: PasswordQuestion) -> Option<Secret>;

    /// Something happened that the person should hear, and that no question follows.
    ///
    /// One method rather than a variant of the others, because it is the only thing here
    /// that expects nothing back: progress while a connection is being made, or a key that
    /// was accepted and then could not be written down. The sentence is complete and
    /// speakable, because it is read aloud exactly as it arrives.
    fn tell(&self, sentence: &str);
}

/// Nobody to ask, so nothing is trusted and nothing is given.
///
/// **The null implementation, written as a type rather than as an absence** — the reasoning
/// [`Plain`](../../../acter_shells/struct.Plain.html) is built on, applied to this port. It
/// is what a launch that names a profile from the environment gets, and what every test
/// whose subject is not the asking gets: there is no window, so a host key that needs a
/// decision is refused and a password that needs typing is not supplied.
///
/// **Refusing is the honest answer rather than a limitation to work around.** A connection
/// that cannot ask cannot be authorised, and the alternative — trusting because nobody was
/// there to object — is exactly the "accept everything" mode decision 3 says Acter does not
/// have.
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

/// What a person is asked about a server's identity, in the order it has to be said.
///
/// **Every field here becomes speech**, which is why the fingerprint is a string rendered
/// the way `ssh-keygen -l` renders it rather than bytes with a formatting decision left to
/// whoever displays it: what a listener compares against is what their hosting provider or
/// their colleague printed, and the two have to be the same characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyQuestion {
    /// The host as the user named it, which is what they will recognise.
    pub host: String,
    /// The port, which is part of the identity: a server on 2222 is a different entry in
    /// `known_hosts` from the same name on 22, and OpenSSH treats them as different hosts.
    pub port: u16,
    /// What this server just offered, as `SHA256:` and unpadded base64 — the form
    /// `ssh-keygen -l` prints and therefore the form a user has to compare against.
    pub fingerprint: String,
    /// Whether anything was recorded for this host before, and what.
    pub state: HostKeyState,
    /// Something true about the answer that is not the answer: a `known_hosts` file that
    /// exists and could not be read, so the user knows this question may be being asked
    /// for a host they have in fact already trusted. `None` when there is nothing to add.
    pub aside: Option<String>,
}

/// What was already recorded for a host, which decides which of two very different
/// questions is being asked.
///
/// **An unknown key and a changed key are not the same dialog** (spec B9, decision 3). The
/// first is routine — every host is unknown once. The second means the server was rebuilt
/// or somebody is sitting between the user and it, and it gets its own words rather than a
/// cheerful "continue?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyState {
    /// Nothing has ever been recorded for this host, on this port.
    Unknown,
    /// Something was recorded, and it is not what the server just offered.
    Changed {
        /// The fingerprint that *was* recorded, in the same form as the offered one, so
        /// the two can be read one after the other and compared character by character.
        recorded: String,
    },
}

/// What the person decided about a host key.
///
/// A two-variant enum rather than a `bool`, because `true` at a call site three files away
/// from this one does not say which way it went, and this is the security decision in SSH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyAnswer {
    /// Connect, and remember this key so the same host does not ask again.
    Accept,
    /// Do not connect. The attempt ends and says so.
    Refuse,
}

/// What a person is asked when the server wants a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordQuestion {
    /// Which host is asking, so a listener answering two connections is never guessing.
    pub host: String,
    /// Which account it is asking about, for the same reason.
    pub user: String,
    /// Whether a password was already tried and refused by the server.
    ///
    /// **Said out loud rather than left to be inferred from the dialog opening twice.** A
    /// second identical prompt with no explanation is indistinguishable from the first one
    /// not having been submitted, which is precisely the confusion this product exists to
    /// remove.
    pub again: bool,
}

/// A password on its way to the far end, and nowhere else.
///
/// **The type is the guarantee, not a comment asking people to be careful.** It has no
/// `Display`, so it cannot be interpolated into a message; its `Debug` prints a fixed
/// placeholder, so it cannot ride into a log, a panic message or a `dbg!`; and it derives
/// no `Serialize`, so it cannot be put on the wire or into the debug event tape that spec
/// A3.2 records event ordering with — a password in a debug tape is a password on disk.
///
/// **It deserializes and does not serialize, which is the asymmetry the product needs.** A
/// password is typed into a dialog and has to reach the backend, so it arrives from the
/// wire; nothing ever sends one the other way, so `Serialize` is absent and the compiler
/// enforces that — including for the debug event recorder, which records what crosses the
/// invoke boundary and therefore has nothing it could record.
///
/// Reading it back is deliberately a call named [`Secret::expose`], so every place that
/// takes the value out is a place a reader can find by searching for that word.
///
/// **What this does not claim**: it does not scrub memory. Rust's `String` can reallocate,
/// and a type that promised erasure it cannot deliver would be worse than one that is clear
/// about its scope. The requirement in spec B9, decision 4 is that the value never reaches
/// the buffer, the announcer, a log or the tape, and that is what these three absences buy.
#[derive(Clone, PartialEq, Eq, serde::Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps a value that has just been typed into a masked field.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for the one caller that has to send it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    /// Says that there is a secret and never what it is. Written out rather than derived,
    /// because deriving would print it.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(not shown)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one behaviour this file has, and it is the one that matters: a password cannot
    /// be printed by the two mechanisms that print things by accident. `Display` and
    /// `Serialize` are absent, which the compiler enforces and no test can; this pins the
    /// third, which is present and could be made to leak by someone deriving it.
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

    /// And the value is still reachable, by the one call that is named after what it does.
    #[test]
    fn the_one_caller_that_has_to_send_it_can_read_it() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
    }

    /// The two host-key situations are different values, so a dialog cannot accidentally
    /// say the routine thing about the serious one. Asserted here rather than trusted,
    /// because the difference is the whole of decision 3.
    #[test]
    fn an_unknown_key_and_a_changed_key_are_different_states() {
        let changed = HostKeyState::Changed {
            recorded: "SHA256:something".to_owned(),
        };

        assert_ne!(HostKeyState::Unknown, changed);
    }
}
