//! Router: the facts the About dialog reads out.
//!
//! **Read from the build rather than typed a second time** (spec A7, decision 3): the
//! version is the one Cargo compiled, so releasing changes what the dialog says with no
//! second edit and no chance of the two disagreeing. The frontend owns the dialog; this
//! owns only the facts.
//!
//! **Since 26 it also says where Acter keeps its settings** (spec 26, decision 5), which is
//! the cheapest answer to "where did that go" for somebody who cannot go looking with a file
//! manager. It is read out of managed state rather than out of the environment, because
//! reading the environment is the composition root's privilege and nobody else's.

use serde::Serialize;
use tauri::State;

use crate::container::{AppState, SettingsFolder};

/// What About says, in the order it says it.
#[derive(Serialize)]
pub(crate) struct About {
    name: &'static str,
    version: &'static str,
    copyright: &'static str,
    licence: &'static str,
    /// The settings folder, as a path a user can read out and type into a file manager.
    settings_folder: String,
    /// Whether Acter is running portable or installed, as a whole sentence.
    settings_standing: &'static str,
}

#[tauri::command]
pub(crate) fn about(state: State<'_, AppState>) -> About {
    facts(&state.settings)
}

/// The facts themselves, given the folder the composition root resolved.
///
/// Separate from the command so the whole answer is assertable without a Tauri runtime; what
/// the runtime adds is the state extraction, and the mock-runtime test in `routers.rs`
/// covers that.
fn facts(settings: &SettingsFolder) -> About {
    About {
        name: "Acter",
        version: env!("CARGO_PKG_VERSION"),
        copyright: "© 2026 Marlon Brandão de Sousa",
        licence: "MIT",
        settings_folder: settings.path.display().to_string(),
        settings_standing: settings.standing.said(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::container::Standing;

    use super::*;

    fn portable() -> SettingsFolder {
        SettingsFolder {
            path: PathBuf::from(r"D:\acter\settings"),
            standing: Standing::Portable,
        }
    }

    /// The version is the build's, which is the whole point of the command existing.
    #[test]
    fn the_version_is_the_one_cargo_compiled() {
        assert_eq!(facts(&portable()).version, env!("CARGO_PKG_VERSION"));
        assert!(
            !facts(&portable()).version.is_empty(),
            "a dialog that reads an empty version is worse than one that reads none"
        );
    }

    #[test]
    fn the_four_facts_are_all_speakable() {
        let about = facts(&portable());
        for fact in [about.name, about.version, about.copyright, about.licence] {
            assert!(!fact.trim().is_empty(), "every fact is said out loud");
        }
    }

    /// **The folder is the one the composition root resolved, not one this router looked
    /// up.** A dialog that read the environment a second time could say a different folder
    /// than the one the files are actually in.
    #[test]
    fn it_says_the_settings_folder_it_was_given() {
        let about = facts(&portable());

        assert_eq!(about.settings_folder, r"D:\acter\settings");
    }

    /// Portable and installed are different sentences, and each is a whole one: this line is
    /// read aloud, and a listener who hears only a path has been told where without being
    /// told why.
    #[test]
    fn portable_and_installed_are_told_apart_in_words() {
        let installed = SettingsFolder {
            path: PathBuf::from(r"C:\Users\someone\AppData\Roaming\acter\settings"),
            standing: Standing::Installed,
        };

        assert_eq!(
            facts(&portable()).settings_standing,
            "Acter is running portable, so its settings are beside the program."
        );
        assert_eq!(
            facts(&installed).settings_standing,
            "Acter is installed, so its settings are kept with your other application data."
        );
    }

    /// Every standing has a sentence, and every sentence ends where the thought does — the
    /// line runs straight into whatever the dialog says next otherwise.
    #[test]
    fn every_standing_is_a_finished_sentence() {
        for standing in [
            Standing::Portable,
            Standing::Installed,
            Standing::Directed,
            Standing::WhereItStarted,
        ] {
            let said = standing.said();
            assert!(said.ends_with('.'), "{said}");
            assert!(
                said.starts_with("Acter") || said.starts_with("This system"),
                "it says who it is about: {said}"
            );
        }
    }
}
