//! Adapter: `ExplainedShells` implements the `Explained` driven port over a plain text file,
//! one shell name per line.
//!
//! Every failure asks again: an unreadable file answers "not explained", and a failed write
//! leaves the shell unrecorded.

use std::fs::{OpenOptions, create_dir_all, read_to_string};
use std::io::Write;
use std::path::PathBuf;

use acter_core::Explained;

pub(crate) struct ExplainedShells {
    file: PathBuf,
}

impl ExplainedShells {
    pub(crate) fn new(file: PathBuf) -> Self {
        Self { file }
    }
}

impl Explained for ExplainedShells {
    fn already(&self, shell: &str) -> bool {
        read_to_string(&self.file)
            .unwrap_or_default()
            .lines()
            .any(|line| line.trim() == shell)
    }

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

    #[test]
    fn a_shell_that_was_remembered_is_not_asked_about_again() {
        let at = directory("remembered");
        let explained = ExplainedShells::new(at.join("explained_shells"));

        assert!(!explained.already("bash"), "nothing has been explained yet");
        explained.remember("bash");

        assert!(explained.already("bash"));
    }

    #[test]
    fn remembering_one_shell_says_nothing_about_another() {
        let at = directory("per-shell");
        let explained = ExplainedShells::new(at.join("explained_shells"));

        explained.remember("bash");

        assert!(explained.already("bash"));
        assert!(!explained.already("zsh"));
        assert!(!explained.already("sh"));
    }

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

    #[test]
    fn a_record_that_does_not_exist_yet_asks_about_everything() {
        let at = directory("absent");
        let explained = ExplainedShells::new(at.join("nowhere").join("explained_shells"));

        assert!(!explained.already("bash"));
    }

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
