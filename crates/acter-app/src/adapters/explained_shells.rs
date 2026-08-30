//! Adapter: `ExplainedShells` implements the `Explained` driven port over a file — the
//! record of which shells this person has had Acter's setup command explained to them for,
//! and has said not to be asked about again.
//!
//! **One line per shell name, in a plain text file**, for the reason `known_hosts` is kept in
//! OpenSSH's own format (spec B9, decision 5): it stays inspectable and deletable with the
//! tools a user already has, and somebody who wants to be asked again can empty it without
//! Acter's help.
//!
//! **Kept per shell and by nothing else** (spec B9.5, decision 10). It is not keyed by host
//! and not keyed by profile: tying it to a connection would re-explain the same command for
//! every new host, which is the thing that teaches people to dismiss dialogs without reading
//! them. Somebody who has read and accepted what Acter runs in bash has not thereby accepted
//! what it runs in zsh — the user's own reason, and a shell whose setup a later entry adds
//! asks again, as it should, because it is a different command.
//!
//! **The path is resolved in the composition root**, which is the one place allowed to read
//! the environment (spec B8, decision 2) — so the whole of this behaviour is testable against
//! a directory made for the test rather than against whatever this machine happens to have.
//!
//! **Every failure is silent and safe in the same direction.** A file that cannot be read
//! answers "not explained", which asks again; a file that cannot be written leaves the
//! preference unrecorded, which asks again. Neither is worth interrupting a connection over,
//! and the cost of the other direction is a command run in somebody's session that nothing
//! ever showed them.

use std::fs::{OpenOptions, create_dir_all, read_to_string};
use std::io::Write;
use std::path::PathBuf;

use acter_core::Explained;

/// The file, and nothing else: it is read on every question and appended to on every answer,
/// so a second window that answers first is seen by this one.
pub(crate) struct ExplainedShells {
    file: PathBuf,
}

impl ExplainedShells {
    pub(crate) fn new(file: PathBuf) -> Self {
        Self { file }
    }
}

impl Explained for ExplainedShells {
    /// Whether this shell is already on the list.
    ///
    /// Read rather than cached, for [`connectable`](acter_core::ConnectApi::connectable)'s
    /// reason: a preference set in another window while this one was open should be true here
    /// the next time it is asked, without a restart. It is asked once per connection, so the
    /// read costs nothing anybody can hear.
    fn already(&self, shell: &str) -> bool {
        read_to_string(&self.file)
            .unwrap_or_default()
            .lines()
            .any(|line| line.trim() == shell)
    }

    /// Adds this shell to the list, unless it is already on it.
    ///
    /// **Appended rather than rewritten**, so nothing this program does can lose a preference
    /// somebody set from another window a moment ago.
    fn remember(&self, shell: &str) {
        if shell.trim().is_empty() || self.already(shell) {
            return;
        }
        if let Some(directory) = self.file.parent() {
            let _ = create_dir_all(directory);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
        {
            let _ = writeln!(file, "{shell}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env::temp_dir;
    use std::fs::{create_dir_all, remove_dir_all, write};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// A directory of this test's own, so nothing here depends on what this machine has or on
    /// what another test wrote.
    fn directory(named: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let at = temp_dir().join(format!(
            "acter-explained-{named}-{}",
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = remove_dir_all(&at);
        create_dir_all(&at).expect("a directory for the test");
        at
    }

    /// The behaviour the dialog turns on: a shell that has been accepted with "do not show
    /// this again" is not asked about again.
    #[test]
    fn a_shell_that_was_remembered_is_not_asked_about_again() {
        let at = directory("remembered");
        let explained = ExplainedShells::new(at.join("explained_shells"));

        assert!(!explained.already("bash"), "nothing has been explained yet");
        explained.remember("bash");

        assert!(explained.already("bash"));
    }

    /// **The whole of decision 10, asserted.** Accepting bash's command says nothing about
    /// zsh's, because it is a different command — so a shell whose setup a later entry adds
    /// asks again.
    #[test]
    fn remembering_one_shell_says_nothing_about_another() {
        let at = directory("per-shell");
        let explained = ExplainedShells::new(at.join("explained_shells"));

        explained.remember("bash");

        assert!(explained.already("bash"));
        assert!(!explained.already("zsh"));
        assert!(!explained.already("sh"));
    }

    /// Appended rather than rewritten, so two windows cannot lose each other's answers — and
    /// remembering the same shell twice does not grow the file.
    #[test]
    fn a_second_answer_joins_the_first_rather_than_replacing_it() {
        let at = directory("appended");
        let file = at.join("explained_shells");
        let explained = ExplainedShells::new(file.clone());

        explained.remember("bash");
        explained.remember("sh");
        explained.remember("bash");

        let kept = read_to_string(&file).expect("the file was written");
        assert_eq!(kept.lines().collect::<Vec<_>>(), ["bash", "sh"]);
    }

    /// **A file that is not there asks, rather than answering from nothing.** This is the
    /// first run of the program, and it is the ordinary case rather than an error.
    #[test]
    fn a_record_that_does_not_exist_yet_asks_about_everything() {
        let at = directory("absent");
        let explained = ExplainedShells::new(at.join("nowhere").join("explained_shells"));

        assert!(!explained.already("bash"));
    }

    /// A file somebody edited by hand is read the way they would expect: one name per line,
    /// blank lines and stray spaces ignored.
    #[test]
    fn a_record_edited_by_hand_reads_as_the_names_it_lists() {
        let at = directory("by-hand");
        let file = at.join("explained_shells");
        write(&file, "  bash  \n\n zsh\n").expect("a record written by hand");
        let explained = ExplainedShells::new(file);

        assert!(explained.already("bash"));
        assert!(explained.already("zsh"));
        assert!(!explained.already("fish"));
    }
}
