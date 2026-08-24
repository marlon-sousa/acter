//! Port (driven): what this particular computer actually has — which WSL distributions
//! exist, and whether a program can be started at all.
//!
//! **The first port about the machine rather than about a session.** Every other driven
//! port is a seam in something already running; this one is asked before anything is
//! running, by the list of things a user may connect to. Running `wsl.exe -l -q` starts a
//! process, depends on what is installed and can fail, which is ARCHITECTURE's classifying
//! question answered three times over: it is I/O, so it is an adapter, so it gets a port.
//!
//! **Both questions, not one each.** "What can this machine run" is a single question the
//! connect list asks once, and two ports answering halves of it would be two fakes to keep
//! in step (spec B5.3, decision 4).
//!
//! Distinct from [`ShellAdapter`](crate::ShellAdapter), which is knowledge: what `cmd.exe`
//! is started with is the same answer on every machine in the world, and which
//! distributions are installed is the same answer on no two machines at all.

use thiserror::Error;

/// What this computer can run.
///
/// `Send + Sync` for [`ShellAdapter`](crate::ShellAdapter)'s reason: the composition root
/// hands one to code running on another task. `&self` throughout because a caller asks a
/// question rather than driving a machine — an implementer that caches is free to, and one
/// that shells out on every call is free to as well.
pub trait InstalledShells: Send + Sync {
    /// Which WSL distributions this machine has, in the order WSL reports them.
    ///
    /// **Everything installed is listed**, `docker-desktop` and the other service
    /// distributions included. Deciding which of a user's distributions is a "real" one is
    /// not knowledge this program has, the guess is invisible to the person it is wrong
    /// for, and a user who wants a shell in that distribution is entitled to it (spec
    /// B5.3, decision 5).
    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions>;

    /// Whether this program can be started at all — how PowerShell 7 is offered only where
    /// it exists, while Windows PowerShell is on every machine.
    fn is_available(&self, program: &str) -> bool;
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
