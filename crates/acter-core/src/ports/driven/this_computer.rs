//! Port (driven): what this computer has — its WSL distributions, the shells an account may
//! log in to, the files a program name resolves to, and the shell a far end's account runs.

use std::path::PathBuf;

use thiserror::Error;

use crate::ShellInstall;

pub trait ThisComputer: Send + Sync {
    /// Every distribution WSL lists, service distributions such as `docker-desktop` included,
    /// in the order WSL lists them.
    fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions>;

    /// Every shell an account here may log in to, the account's own first.
    fn login_shells(&self) -> Vec<LoginShell>;

    /// Every install of this program, most preferred first; empty means not installed.
    ///
    /// Implementers look files up and never start them: this is asked while a list is built.
    fn installs(&self, program: &str) -> Vec<ShellInstall>;

    /// The login shell of the named far end's account, or of the default one for `None`.
    ///
    /// `None` means nothing could be learned; implementers answer within a deadline, because
    /// this is asked before the session's first prompt.
    fn login_shell(&self, distribution: Option<&str>) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginShell {
    pub install: ShellInstall,
    /// Whether this is the shell the account itself logs in to.
    pub default: bool,
}

impl LoginShell {
    pub fn name(&self) -> String {
        self.install
            .program
            .file_name()
            .map_or_else(String::new, |file| file.to_string_lossy().into_owned())
    }

    pub fn program(&self) -> &PathBuf {
        &self.install.program
    }
}

/// Why there is no WSL entry to offer; each message is a whole spoken sentence.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NoDistributions {
    #[error(
        "Windows Subsystem for Linux is not installed on this computer, so there are no \
         Linux distributions to connect to."
    )]
    NotInstalled,

    /// `detail` is WSL's own sentence, spoken after ours.
    #[error(
        "Windows Subsystem for Linux is installed, but it could not list its \
         distributions. {detail}"
    )]
    NotWorking { detail: String },

    #[error(
        "Windows Subsystem for Linux is installed, but no Linux distribution has been \
         added to it yet."
    )]
    NoneInstalled,
}

#[cfg(test)]
mod tests {
    use super::*;

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
