//! Port (driven): what this particular computer actually has — which WSL distributions
//! exist, which shells an account here may log in to, which files a named program actually
//! resolves to, and which shell a far end's own account runs.
//!
//! **The first port about the machine rather than about a session.** Every other driven
//! port is a seam in something already running; this one is asked before anything is
//! running, by the list of things a user may connect to. Running `wsl.exe -l -q` starts a
//! process, depends on what is installed and can fail, which is ARCHITECTURE's classifying
//! question answered three times over: it is I/O, so it is an adapter, so it gets a port.
//!
//! **Every question here, not one port each.** "What can this machine run" is a single
//! question, and ports answering thirds of it would be three fakes to keep in step (spec
//! B5.3, decision 4). B5.5 added the third for that reason rather than opening a seam: which
//! shell a distribution runs is the same kind of question about the same machine, asked of
//! the same client (spec B5.5, decision 2).
//!
//! # It was called `ThisComputer` until M2, and the name is what had outgrown itself
//!
//! A Mac cannot answer [`wsl_distributions`](ThisComputer::wsl_distributions) and a Windows
//! machine cannot answer [`login_shells`](ThisComputer::login_shells), and a port whose
//! implementers each decline a method looks like a port that should have been split. It is
//! not, and the reason is what each of those answers actually *is* (spec M2, decision 1).
//!
//! **A platform that lacks one of these states a fact rather than refusing one.** There is
//! no Windows Subsystem for Linux on a Mac, so `NotInstalled` is true there in the plainest
//! sense; there is no `/etc/shells` on Windows, so an empty list is true there. Neither is
//! ever *asked*, because the catalogue offers no `Wsl` kind on macOS and no `Terminal` kind
//! on Windows — but an implementer forced to write a body writes a true one, which is not
//! what a refusal means.
//!
//! **And the split these questions really have is not per platform.** `installs` is asked
//! everywhere; `login_shells` is POSIX rather than Apple's, so Linux will answer it with the
//! same file and the same code; and only `wsl_distributions` belongs to one operating
//! system. Splitting by platform would have put a Unix question in a macOS port and then
//! moved it again the day Linux arrived.
//!
//! **One of these is not asked while the list is built.** `login_shell` runs per connection,
//! because asking per row would start one `wsl.exe` for every distribution every time the
//! connect dialog opens.
//!
//! Distinct from [`ShellAdapter`](crate::ShellAdapter), which is knowledge: what `cmd.exe`
//! is started with is the same answer on every machine in the world, and which
//! distributions are installed is the same answer on no two machines at all.

use std::path::PathBuf;

use thiserror::Error;

use crate::ShellInstall;

/// What this computer can run.
///
/// `Send + Sync` for [`ShellAdapter`](crate::ShellAdapter)'s reason: the composition root
/// hands one to code running on another task. `&self` throughout because a caller asks a
/// question rather than driving a machine — an implementer that caches is free to, and one
/// that shells out on every call is free to as well.
pub trait ThisComputer: Send + Sync {
    /// Which WSL distributions this machine has, in the order WSL reports them.
    ///
    /// **Everything installed is listed**, `docker-desktop` and the other service
    /// distributions included. Deciding which of a user's distributions is a "real" one is
    /// not knowledge this program has, the guess is invisible to the person it is wrong
    /// for, and a user who wants a shell in that distribution is entitled to it (spec
    /// B5.3, decision 5).
    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions>;

    /// Every shell an account on this computer may log in to, the account's own first.
    ///
    /// **`/etc/shells` is the list, and it is the list a Mac itself uses** (spec M2,
    /// decision 3): it is what `chpass` will accept, what Terminal.app's preferences offer,
    /// and therefore the set of shells a user of this machine already believes they have.
    /// Enumerating `/bin` instead would offer programs nobody chose, and reading the
    /// account's own shell alone would hide the six others this Mac ships.
    ///
    /// **Files rather than names, for [`installs`](Self::installs)' reason.** Every entry is
    /// an absolute path already, so there is nothing to resolve and nothing to resolve
    /// *twice* — the file listed is the file verified and the file started (spec B5.7,
    /// decision 1).
    ///
    /// **The account's own shell is first and says so.** Enter on the row with nothing
    /// chosen has to start what a Terminal.app window would have started, and a listener
    /// arrowing the panel meets their own shell before six they have never used.
    ///
    /// **An empty list is one situation rather than three**, which is what makes this
    /// different from [`wsl_distributions`](Self::wsl_distributions). WSL can be absent,
    /// broken or empty and those are three different sentences; a Unix machine with no shell
    /// an account may log in to is a machine that is broken in one way, and the kind's own
    /// instructions are what say so.
    fn login_shells(&self) -> Vec<LoginShell>;

    /// Every install of this program this machine has, most preferred first.
    ///
    /// **It answers with the files it found rather than with a boolean, since B5.7**
    /// (decision 1). The old shape asked whether a *name* could be started and the transport
    /// later started that same name, so Windows resolved it a second time and nothing
    /// guaranteed the two resolutions landed on the same file. Verifying a signature under
    /// that regime would be theatre. Resolve once, verify that file, start that file.
    ///
    /// **An empty list is what "not installed" means now** — how PowerShell 7 is offered
    /// only where it exists, while Windows PowerShell is on every machine.
    ///
    /// **`PATH` is one source and not the whole of it** (decision 2). An MSI install with
    /// "add to PATH" unchecked is invisible to `PATH`; scoop and chocolatey put shims on it
    /// that are not the program; and on the developer's own machine `PATH` yields two hits
    /// for one install. What `PATH` alone knows, and no other source does, is what the name
    /// means to *this* user — so the entry it resolves first is marked
    /// [`PathStanding::First`](crate::PathStanding) rather than merely included.
    ///
    /// **Looked up rather than run**, which B5.3 decided and this keeps: the question is
    /// asked while building a list of things the user *may* connect to, and starting each
    /// candidate to find out whether it starts would open sessions nobody asked for. That is
    /// also why no version is read off any file (decision 3).
    fn installs(&self, program: &str) -> Vec<ShellInstall>;

    /// Which shell a WSL distribution's own account is configured to run, by name, or
    /// `None` when nothing honest can be said about it.
    ///
    /// `None` for the distribution names whatever WSL calls the default, which leaves that
    /// choice to WSL rather than making it here — the same reason `wsl.exe` is started with
    /// no `-d` when nobody named one.
    ///
    /// **`wsl.exe` is a client, not a shell** (spec B5.5). What it starts is whatever login
    /// shell the distribution's account carries in its own passwd entry, and until B5.5
    /// this program assumed that was bash — injecting a `PROMPT_COMMAND` program into
    /// accounts running zsh, fish or dash, where nothing would ever read it.
    ///
    /// **Advisory, never a gate** (decision 3). Every way of failing — a distribution that
    /// will not start, a client that is not there, an answer that is not a shell name, a
    /// deadline that passed — is the same `None`, and `None` costs the session nothing: it
    /// starts anyway, unintegrated and unnamed. An implementer runs this with a deadline
    /// rather than waiting on it, because it sits in the seconds before there is a prompt
    /// and those are already the worst seconds in this product (roadmap 23.7).
    ///
    /// **The name licenses saying so and nothing else.** Knowing a distribution runs zsh
    /// earns the word "zsh" in the sentence a listener hears; it does not earn a zsh
    /// injection, which nobody has measured the way B5.3 measured bash's.
    ///
    /// **Asked once per connection, not once per row** (decision 3). Asking while the
    /// connect list is built would mean one `wsl.exe` for every distribution every time the
    /// dialog opens, which is worse in the very place a listener is already waiting.
    fn login_shell(&self, distribution: Option<&str>) -> Option<String>;
}

/// One shell an account on this computer may log in to.
///
/// **The default is a fact carried per entry rather than an index into the order.** The
/// order is `/etc/shells`' own with the account's shell moved to the front, and a list that
/// says "the first one is special" is a list that lies the moment anything sorts or filters
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShell {
    /// The file this entry is, found once and started as found.
    pub install: ShellInstall,
    /// Whether this is the shell the account itself logs in to — the one a Terminal.app
    /// window would have started.
    pub default: bool,
}

impl LoginShell {
    /// What this shell is called: the file's own name, which is what a user of this machine
    /// calls it and what [`setup_for`](crate::SessionSetup) is keyed by.
    ///
    /// The full path is not the name. `/bin/zsh` read aloud is four sounds a listener has to
    /// assemble before hearing the one that identifies it, and the panel's job is to be
    /// arrowed quickly.
    pub fn name(&self) -> String {
        self.install
            .program
            .file_name()
            .map_or_else(String::new, |file| file.to_string_lossy().into_owned())
    }

    /// The file itself, which is what gets verified and what gets started.
    pub fn program(&self) -> &PathBuf {
        &self.install.program
    }
}

/// Why there is no WSL entry to offer.
///
/// **Three situations rather than an empty list**, because they need three different
/// sentences: a user who has never installed WSL, a user whose WSL is broken and a user
/// who has WSL but no distribution in it are owed different things, and none of them is
/// served by a list that silently omits what they expected to see (spec B5.3, decision 6).
///
/// The `Display` strings are whole spoken sentences rather than the lowercase fragments
/// `thiserror` conventionally carries, for [`TransportError`](crate::TransportError)'s
/// reason: each of these reaches a screen reader user as the answer to "why is Linux not
/// in this list", and CLAUDE.md makes that a domain requirement rather than polish.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NoDistributions {
    /// `wsl.exe` could not be started, so the feature is not on this machine at all.
    #[error(
        "Windows Subsystem for Linux is not installed on this computer, so there are no \
         Linux distributions to connect to."
    )]
    NotInstalled,

    /// `wsl.exe` ran and refused. `detail` is WSL's own sentence, appended as its own
    /// rather than spliced into ours, so a reader reaches the plain-language part first.
    #[error(
        "Windows Subsystem for Linux is installed, but it could not list its \
         distributions. {detail}"
    )]
    NotWorking { detail: String },

    /// `wsl.exe` ran, succeeded, and named nothing. WSL is present and empty.
    #[error(
        "Windows Subsystem for Linux is installed, but no Linux distribution has been \
         added to it yet."
    )]
    NoneInstalled,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, so a new one cannot be added without deciding what it says.
    fn every_variant() -> Vec<NoDistributions> {
        vec![
            NoDistributions::NotInstalled,
            NoDistributions::NotWorking {
                detail: "The Windows Subsystem for Linux optional component is not enabled."
                    .to_owned(),
            },
            NoDistributions::NoneInstalled,
        ]
    }

    /// The one behavior this file has, and the one worth pinning: these strings are
    /// spoken. A fragment like "no distributions" would reach a listener with no subject
    /// and no advice, which is the failure this test exists to prevent.
    #[test]
    fn every_reason_speaks_a_whole_sentence() {
        for reason in every_variant() {
            let spoken = reason.to_string();
            let first = spoken.chars().next().expect("a message is never empty");
            assert!(
                first.is_uppercase(),
                "a spoken message starts a sentence: {spoken}"
            );
            assert!(
                spoken.ends_with('.'),
                "a spoken message ends in a full stop, so a reader pauses: {spoken}"
            );
            assert!(
                spoken.split_whitespace().count() >= 5,
                "a spoken message says what happened, not a label: {spoken}"
            );
        }
    }

    /// The three are told apart by what a listener hears, not by a discriminant: a user
    /// who has never installed WSL and one whose WSL is broken must not be read the same
    /// sentence.
    #[test]
    fn the_three_situations_are_three_different_sentences() {
        let spoken: Vec<String> = every_variant().iter().map(ToString::to_string).collect();

        for (index, one) in spoken.iter().enumerate() {
            for other in &spoken[index + 1..] {
                assert_ne!(one, other, "each situation is said differently");
            }
        }
    }

    #[test]
    fn a_broken_wsl_carries_its_own_words_after_the_plain_language_part() {
        let reason = NoDistributions::NotWorking {
            detail: "Please enable the Virtual Machine Platform feature.".to_owned(),
        };
        let spoken = reason.to_string();

        assert!(spoken.starts_with("Windows Subsystem for Linux is installed"));
        assert!(spoken.ends_with("Please enable the Virtual Machine Platform feature."));
    }
}
