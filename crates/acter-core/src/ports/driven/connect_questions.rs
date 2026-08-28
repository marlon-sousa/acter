//! Port (driven): everything one attempt to connect may have to ask the person in front of
//! the window, before there is a session to ask it in.
//!
//! **It is [`SshQuestions`] plus the one question that is not about a far end.** B9
//! established the shape — a question goes out on the conversation's channel and the
//! connection parks until an answer comes back — for two things a server can stop on: a host
//! key nobody has seen, and a password nobody has typed. B5.7 adds a third, and it is asked
//! about this machine rather than about the other one: the file that is about to be started
//! did not verify, and starting it is a decision only the user can make (spec B5.7,
//! decision 6).
//!
//! **A supertrait rather than a fourth method on `SshQuestions`.** The SSH transport is
//! handed an asker and must not be handed a question it can never ask: what it needs is the
//! two questions a server raises, and it is measured against a real server with no window
//! anywhere near it. What the *connect service* needs is all three, so it takes this and
//! passes the SSH half down.
//!
//! Sync and dyn-compatible, per ARCHITECTURE's rule for port traits. Blocking is the point
//! of it, for [`SshQuestions`]' reason: the thing that waits is a task of the connection's
//! own, never an invoke, which Tauri runs on the main thread and which would deadlock the
//! answer it is waiting for.

use crate::{SshQuestions, Verdict};

/// The questions an attempt to connect asks, of which SSH's two are the first.
pub trait ConnectQuestions: SshQuestions {
    /// The file this machine is about to start did not verify, and here is what was found.
    ///
    /// **Never a gate, and the default is not to start** (decision 6). Everything this
    /// machine has stays in the list — a self-built pwsh, a corporate re-signed build, a
    /// damaged catalog database and an offline revocation check are all legitimate and all
    /// common — and what changes is that the user is told before the program runs rather
    /// than after. The safe answer is the one that does nothing, for the reason
    /// [`SshQuestions::host_key`] refuses by default.
    fn unverified(&self, question: ProgramQuestion) -> ProgramAnswer;
}

/// What a person is asked about a file that would not verify, in the order it has to be
/// said.
///
/// **Every field here becomes speech.** The verdict arrives whole rather than as a sentence
/// somebody assembled at the call site, so the words a listener hears are decided in one
/// place — [`Verdict::said`] — and the dialog can also put the path and the signer somewhere
/// they can be read character by character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramQuestion {
    /// What the user chose, as they heard it in the list: `PowerShell 7 (Microsoft Store)`.
    pub label: String,
    /// The file that would be started, in full. **The full path rather than the name**,
    /// because the thing this check defeats is `PATH`-order hijacking, and which directory
    /// the file is in is the whole of what a user needs to recognise it as wrong.
    pub program: String,
    /// What verifying it found.
    pub verdict: Verdict,
}

/// What the person decided about starting it.
///
/// A two-variant enum rather than a `bool`, for [`HostKeyAnswer`](crate::HostKeyAnswer)'s
/// reason: `true` at a call site three files away does not say which way it went, and this
/// is a security decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramAnswer {
    /// Start it anyway. The attempt goes on, and what was agreed to is said out loud.
    Start,
    /// Do not. The attempt ends and says why, and whatever session was running is untouched.
    DoNotStart,
}

/// Nobody to ask, so nothing unverified is started.
///
/// The same refusal [`Unasked`](crate::Unasked) makes about a host key, and for the same
/// reason: a launch that names a profile from the environment has no window to put a
/// question in, and starting a file nobody could be asked about is the "accept everything"
/// mode Acter does not have.
impl ConnectQuestions for crate::Unasked {
    fn unverified(&self, _question: ProgramQuestion) -> ProgramAnswer {
        ProgramAnswer::DoNotStart
    }
}

#[cfg(test)]
mod tests {
    use crate::{Fault, Unasked};

    use super::*;

    /// The null asker's one behaviour, and the one that matters: with nobody to ask, nothing
    /// unverified starts. A `Start` here would mean a launch from the environment quietly
    /// running whatever `PATH` resolved to.
    #[test]
    fn with_nobody_to_ask_nothing_unverified_is_started() {
        let answer = Unasked.unverified(ProgramQuestion {
            label: "PowerShell 7".to_owned(),
            program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
            verdict: Verdict::Untrusted {
                fault: Fault::NotSigned,
            },
        });

        assert_eq!(answer, ProgramAnswer::DoNotStart);
    }
}
