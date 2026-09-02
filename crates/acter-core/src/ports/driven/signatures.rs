//! Port (driven): who signed a file, and whether this computer trusts the answer.
//!
//! **The second port about the machine rather than about a session**, after
//! [`ThisComputer`](crate::ThisComputer) — and the one place in the product where
//! being wrong is a security answer. It is a port for ARCHITECTURE's classifying question
//! answered three times over: verifying a signature reads the filesystem, reads this
//! machine's certificate stores, and can reach the network for a revocation list.
//!
//! **Asked when a program is about to be started, not when the list is drawn** (spec B5.7,
//! decision 7). `WinVerifyTrust` is not free and revocation can reach the network; the
//! Connect dialog builds its list on open, in front of a listener who is waiting, and this
//! must not put a network timeout there — which is 23.7's objection applied one entry later.
//! So the list keeps the cheap discovery and the verifying happens once, on the connection,
//! where B9 already established a conversation with steps in it.
//!
//! **Refusing to answer is an ordinary outcome, not a failure** (decision 4). An execution
//! alias cannot be opened at all; a revocation check can time out. Both produce
//! [`Verdict::Unverifiable`](crate::Verdict::Unverifiable) with a reason rather than a
//! trusted or an untrusted verdict, because a listener who cannot see the file is owed the
//! difference between "nobody signed this" and "Acter could not tell".

use std::path::Path;

use crate::Verdict;

/// What signed the files this machine would start.
///
/// `Send + Sync` for [`ThisComputer`](crate::ThisComputer)'s reason: the composition
/// root hands one to code running on another task. `&self` throughout because a caller asks
/// a question — an implementer that caches is free to, and decision 7 says it should.
pub trait Signatures: Send + Sync {
    /// Verify this exact file now, and say what Windows made of it.
    ///
    /// **This file, resolved once, rather than a name** (decision 1). A verification that
    /// took a program name would resolve it a second time, and would then be verifying one
    /// file while the transport started whichever file `PATH` happened to name a moment
    /// later — which is theatre rather than a check.
    fn verdict(&self, program: &Path) -> Verdict;

    /// What is already known about this file, without verifying anything and without
    /// touching the network.
    ///
    /// **How the connect list can name a verdict it did not pay for** (decisions 6 and 7).
    /// An entry that failed to verify when somebody last tried to start it carries that in
    /// its name the way A11's missing edition does — and an entry nobody has tried yet
    /// answers `None`, which is a list that says nothing rather than a list that stalls.
    fn known(&self, program: &Path) -> Option<Verdict>;
}

/// Nothing checked, and nothing claimed.
///
/// **The null implementation, written as a type rather than as an absence** — the reasoning
/// [`Unasked`](crate::Unasked) is built on, applied to this port. It is what a build with no
/// signature adapter for its platform gets, and what a test whose subject is not the checking
/// gets.
///
/// **It answers unverifiable rather than trusted**, which is the same refusal `Unasked`
/// makes: a file nobody checked is not a file anybody vouched for, and the alternative —
/// trusting because there was nothing on this platform to ask — is exactly the "accept
/// everything" mode Acter does not have.
pub struct Unchecked;

impl Signatures for Unchecked {
    fn verdict(&self, _program: &Path) -> Verdict {
        Verdict::Unverifiable {
            why: "This build of Acter cannot check signatures on this operating system.".to_owned(),
        }
    }

    fn known(&self, _program: &Path) -> Option<Verdict> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// The null implementation's one behaviour, and the one worth pinning: it does not
    /// vouch for anything. A `Trusted` here would silently switch the whole product's
    /// answer on a platform nobody wrote an adapter for yet.
    #[test]
    fn checking_nothing_vouches_for_nothing() {
        let verdict = Unchecked.verdict(&PathBuf::from(r"C:\Windows\system32\cmd.exe"));

        assert!(matches!(verdict, Verdict::Unverifiable { .. }));
        assert!(
            !verdict.settled(),
            "so the connection asks before starting it"
        );
    }

    /// And it remembers nothing, so a list built over it names no verdicts at all.
    #[test]
    fn checking_nothing_remembers_nothing() {
        assert_eq!(
            Unchecked.known(&PathBuf::from(r"C:\Windows\system32\cmd.exe")),
            None
        );
    }
}
