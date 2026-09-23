//! Port (driven): who signed a file, and whether this computer trusts the answer.

use std::path::Path;

use crate::Verdict;

pub trait Signatures: Send + Sync {
    /// Can reach the network, so it is asked only when a program is about to start; a file that
    /// cannot be opened or a revocation check that times out is [`Verdict::Unverifiable`].
    fn verdict(&self, program: &Path) -> Verdict;

    /// What is already known without verifying anything; `None` when this file, as it stands
    /// now, has not been verified.
    fn known(&self, program: &Path) -> Option<Verdict>;
}

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

    #[test]
    fn checking_nothing_vouches_for_nothing() {
        let verdict = Unchecked.verdict(&PathBuf::from(r"C:\Windows\system32\cmd.exe"));

        assert!(matches!(verdict, Verdict::Unverifiable { .. }));
        assert!(
            !verdict.settled(),
            "so the connection asks before starting it"
        );
    }

    #[test]
    fn checking_nothing_remembers_nothing() {
        assert_eq!(
            Unchecked.known(&PathBuf::from(r"C:\Windows\system32\cmd.exe")),
            None
        );
    }
}
