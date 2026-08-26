//! Adapter: asking the far end what it is, on a channel of its own, before the session
//! exists (spec B9, decision 7).
//!
//! **Between authentication and the session channel**, which is a window that exists
//! because SSH finishes authenticating at the *protocol* level, before any channel is
//! opened: only `pty-req` plus a `shell` request makes sshd fork the login shell. So there
//! is a moment when Acter is signed in and no shell exists yet, and that is where this
//! goes. Asking first is what the code needs rather than what is tidy —
//! `SessionService::start` takes `ShellFacts` **by value** and reads them at construction,
//! so an answer arriving after the session existed would leave two options: build it with
//! facts known to be wrong, or make the facts mutable and undo the reason they travel
//! together.
//!
//! **A second channel, never a line typed into the session.** An `exec` request on a fresh
//! channel produces output that never reaches the terminal buffer. Typing the probe into
//! the session instead would put a command nobody typed in front of a screen reader, which
//! is B4.9's whole subject.
//!
//! **It does not make the session integrated.** Knowing the far end is bash does not make
//! bash emit OSC 133; without a snippet installed there, nothing does. What this changes is
//! what Acter can *say* and what it *knows* — a sentence with a subject, and a correct
//! end-of-input — not what the far end emits.
//!
//! **Advisory, never a gate.** `exec` can be refused: a server with `ForceCommand`, a
//! restricted shell, or `internal-sftp` alone will not run it, and under `ForceCommand` it
//! can answer about something else entirely. Every failure here is [`FarEnd::unknown`], the
//! session opens regardless, and the deadline is short — the user's shell is never waiting
//! on our curiosity.

use std::time::Duration;

use russh::{ChannelMsg, client};
use tokio::time::timeout;

/// What the far end said about itself.
///
/// **Three states, and they are three different sentences** (decision 7): a shell that has
/// been *measured* is integrated; one that is known of but unmeasured is unintegrated and
/// *named*; an unrecognised shell, or a probe that failed, is unintegrated with no name.
/// This type carries only what was said — which of the three it becomes is shell knowledge
/// and belongs to whoever maps a name onto facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FarEnd {
    /// `$SHELL` as sshd handed it to the command: the account's login shell, from its
    /// passwd entry.
    pub shell: Option<String>,
    /// Which shell said so itself, from the version variable it sets: the most certain
    /// evidence there is, because a shell is the only thing that sets its own.
    pub flavour: Option<String>,
}

impl FarEnd {
    /// Nothing was learned, which is a state the product supports rather than an error.
    pub fn unknown() -> Self {
        Self::default()
    }

    /// What to call this far end, best evidence first, or `None` when there is nothing
    /// honest to say.
    ///
    /// The version variable wins over `$SHELL` because it is the shell's own account of
    /// itself, where `$SHELL` is the account's *configured* shell and can be out of date
    /// with what is actually running.
    pub fn name(&self) -> Option<String> {
        if let Some(flavour) = &self.flavour {
            return Some(flavour.clone());
        }
        self.shell.as_deref().and_then(basename)
    }
}

/// What is asked, and it is asked in one line so one round trip answers everything.
///
/// **`$0` is deliberately not in it, against the spec's wording, and the reason is fish.**
/// Decision 7 lists `$SHELL`, then `$0`, then the version variables. `$0` is a parse error
/// in fish — variable names cannot begin with a digit — and a parse error takes the *whole*
/// command with it, so asking for it would cost the other two answers on exactly the shell
/// least likely to be recognised by any other means. It also adds nothing here: what `$0`
/// established on the rig is that a `shell` request starts a *login* shell (`-bash`) while
/// an `exec` request does not (`bash`), and this probe only ever runs on an `exec` channel,
/// where its answer is the basename `$SHELL` already gave.
///
/// `printf` rather than `echo`, because `echo`'s handling of backslashes and flags differs
/// between these very shells; the brackets make an empty answer distinguishable from a
/// missing one.
const ASK: &str = "printf 'ACTER SHELL=[%s] BASH=[%s] ZSH=[%s] FISH=[%s]\\n' \
                   \"$SHELL\" \"$BASH_VERSION\" \"$ZSH_VERSION\" \"$FISH_VERSION\"";

/// How long the far end has to answer before it is left behind.
///
/// **Short and fixed** (decision 7). This is the one place a probe could add time to the
/// seconds before a prompt, and those seconds are already the worst in the product
/// (roadmap 23.7), so an unanswered probe is abandoned rather than waited on. It is a
/// deadline for a machine printing one line down a connection that has just carried a key
/// exchange and an authentication — not a network timeout.
pub(crate) const PATIENCE: Duration = Duration::from_secs(3);

/// Asks, and answers `unknown` for every way that can fail to work.
pub(crate) async fn ask<H: client::Handler>(
    connection: &mut client::Handle<H>,
    patience: Duration,
) -> FarEnd {
    match timeout(patience, exec(connection)).await {
        Ok(Some(said)) => read(&said),
        // A refused `exec`, a channel that would not open, a server that said nothing, or a
        // deadline that passed: all one answer, because all of them mean the same thing to
        // everything above — Acter does not know what this far end is.
        Ok(None) | Err(_) => FarEnd::unknown(),
    }
}

/// Opens a channel, runs the question on it, and reads until the far end is done.
async fn exec<H: client::Handler>(connection: &mut client::Handle<H>) -> Option<String> {
    let mut channel = connection.channel_open_session().await.ok()?;
    // No pty is requested, deliberately: this is a command and not a terminal, and asking
    // for one would make sshd run the account's *login* shell instead — the thing that
    // reads `~/.profile` and draws a prompt, neither of which is wanted here.
    channel.exec(true, ASK).await.ok()?;

    let mut said = String::new();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => said.push_str(&String::from_utf8_lossy(&data)),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Some(said)
}

/// Reads the one line the far end printed, and refuses to guess at anything else.
fn read(said: &str) -> FarEnd {
    FarEnd {
        shell: field(said, "SHELL").filter(|value| is_a_path(value)),
        flavour: flavour(said),
    }
}

/// Which shell set a version variable of its own, which is the most certain evidence there
/// is: no other program sets `$BASH_VERSION`.
fn flavour(said: &str) -> Option<String> {
    [("BASH", "bash"), ("ZSH", "zsh"), ("FISH", "fish")]
        .into_iter()
        .find(|(variable, _)| field(said, variable).is_some_and(|value| !value.is_empty()))
        .map(|(_, name)| name.to_owned())
}

/// One `NAME=[value]` out of the answer.
fn field(said: &str, name: &str) -> Option<String> {
    let at = said.find(&format!("{name}=["))? + name.len() + 2;
    let rest = said.get(at..)?;
    let end = rest.find(']')?;
    Some(rest[..end].to_owned())
}

/// Whether this looks like the path to a program rather than something else entirely.
///
/// **An answer that is not a path is itself a useful signal** (decision 7). Windows OpenSSH
/// defaulting to `cmd` echoes `$SHELL` back literally, and a `ForceCommand` can answer about
/// something else; both fall into "unrecognised" rather than being parsed hopefully into a
/// name Acter would then say out loud.
fn is_a_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('$')
        && !value.contains(char::is_whitespace)
        && basename(value).is_some()
}

/// The program's own name, without the directories and without a login shell's leading `-`.
fn basename(path: &str) -> Option<String> {
    let file = path.rsplit(['/', '\\']).next()?;
    let file = file.strip_prefix('-').unwrap_or(file);
    (!file.is_empty()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rig's own answer for the `acter` account, copied from what it actually printed.
    const BASH: &str = "ACTER SHELL=[/bin/bash] BASH=[5.2.15(1)-release] ZSH=[] FISH=[]\n";
    /// And for `dashuser`, which is the control: a shell that sets no version variable of
    /// its own, so `$SHELL` is the only evidence and it can only have come from sshd.
    const DASH: &str = "ACTER SHELL=[/bin/dash] BASH=[] ZSH=[] FISH=[]\n";

    #[test]
    fn a_shell_that_names_itself_is_taken_at_its_word() {
        let far_end = read(BASH);

        assert_eq!(far_end.flavour.as_deref(), Some("bash"));
        assert_eq!(far_end.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(far_end.name().as_deref(), Some("bash"));
    }

    /// **The control.** `dash` sets no version variable, so the name comes from `$SHELL`
    /// alone — which is exactly the case the probe exists for, and the one that proves the
    /// value is sshd's rather than a shell's invention.
    #[test]
    fn a_shell_that_names_nothing_is_named_by_its_path() {
        let far_end = read(DASH);

        assert_eq!(far_end.flavour, None, "dash sets no version variable");
        assert_eq!(far_end.name().as_deref(), Some("dash"));
    }

    /// A far end that answered with something other than a path is unrecognised rather than
    /// hopefully parsed: Windows OpenSSH defaulting to cmd echoes the variable back.
    #[test]
    fn an_answer_that_is_not_a_path_names_nothing() {
        let far_end = read("ACTER SHELL=[$SHELL] BASH=[] ZSH=[] FISH=[]");

        assert_eq!(far_end.shell, None);
        assert_eq!(far_end.name(), None, "nothing is said rather than nonsense");
    }

    /// A server that refused `exec`, or said nothing at all, is the same three-state answer
    /// as one that answered unhelpfully — and it is never an error.
    #[test]
    fn a_far_end_that_said_nothing_is_simply_unknown() {
        assert_eq!(read(""), FarEnd::unknown());
        assert_eq!(FarEnd::unknown().name(), None);
    }

    /// The version variable wins, because it is the shell's own account of itself where
    /// `$SHELL` is only what the account was configured with.
    #[test]
    fn what_is_running_beats_what_was_configured() {
        let far_end = read("ACTER SHELL=[/bin/sh] BASH=[5.2.15(1)-release] ZSH=[] FISH=[]");

        assert_eq!(far_end.name().as_deref(), Some("bash"));
    }

    /// A login shell's `$SHELL` is a path; the name spoken is the program, never the path,
    /// because a path read aloud is a string of directory names with the useful word last.
    #[test]
    fn the_name_is_the_program_rather_than_the_path_to_it() {
        assert_eq!(basename("/usr/local/bin/fish").as_deref(), Some("fish"));
        assert_eq!(
            basename("-bash").as_deref(),
            Some("bash"),
            "a login shell's leading dash is not part of its name"
        );
        assert_eq!(basename(""), None);
    }
}
