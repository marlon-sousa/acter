//! Adapter: the About Tauri router — the facts the About dialog reads, as one
//! `#[tauri::command]`.
//!
//! **Since 26 it says where Acter keeps its settings and what this build is** (spec 26,
//! decision 5). The folder is the cheapest answer to "where did that go" for somebody who
//! cannot go looking with a file manager, and the version stopped being Cargo's the moment
//! the build started stamping the real one.
//!
//! **Everything here comes out of the settings object through managed state**, and the
//! dialog asks the environment nothing: a second lookup could name a folder the files are
//! not in.

use serde::Serialize;
use tauri::State;

use crate::adapters::Settings;
use crate::container::AppState;

/// What About says, in the order it says it.
#[derive(Serialize)]
pub(crate) struct About {
    name: &'static str,
    /// What a bug report carries: `1.0.0`, or `development-521c956`.
    ///
    /// **Kept beside the sentence rather than replaced by it.** The dialog is copyable
    /// text, so the identifier has to be there to be copied; what is *read out* is
    /// [`Self::version_said`], because `development-521c956` is a value and not a sentence.
    version: String,
    /// The version as a listener hears it: "Version 1.0.0." or "Development build, commit
    /// 521c956."
    version_said: String,
    copyright: &'static str,
    licence: &'static str,
    /// The settings folder, as a path a user can read out and type into a file manager.
    settings_folder: String,
    /// How Acter came to be using it, as a whole sentence.
    settings_standing: &'static str,
}

#[tauri::command]
pub(crate) fn about(state: State<'_, AppState>) -> About {
    facts(&state.settings)
}

/// The facts themselves, given the settings object the composition root built.
///
/// Separate from the command so the whole answer is assertable without a Tauri runtime;
/// what the runtime adds is the state extraction, and the mock-runtime test in `routers.rs`
/// covers that.
fn facts(settings: &Settings) -> About {
    About {
        name: "Acter",
        version: settings.version().identifier.clone(),
        version_said: settings.version().said.clone(),
        copyright: "© 2026 Marlon Brandão de Sousa",
        licence: "MIT",
        settings_folder: settings.folder().display().to_string(),
        settings_standing: settings.standing().said(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::container::{SettingsFolder, Standing, Version};

    use super::*;

    /// A settings object over a folder nothing was written to, which is all these need:
    /// the runtime values are set in the constructor and no test here touches the
    /// document.
    fn about_a(standing: Standing, version: Version) -> About {
        facts(&Settings::open(
            SettingsFolder {
                path: PathBuf::from(r"D:\acter\settings"),
                standing,
            },
            version,
        ))
    }

    #[test]
    fn the_four_original_facts_are_all_speakable() {
        let about = about_a(Standing::Portable, Version::released("1.0.0"));

        for fact in [
            about.name,
            about.version.as_str(),
            about.copyright,
            about.licence,
        ] {
            assert!(!fact.trim().is_empty(), "every fact is said out loud");
        }
    }

    /// **The folder is the one the composition root resolved, not one this router looked
    /// up.** A dialog that read the environment a second time could say a different folder
    /// than the one the files are actually in.
    #[test]
    fn it_says_the_settings_folder_it_was_given() {
        let about = about_a(Standing::Portable, Version::released("1.0.0"));

        assert_eq!(about.settings_folder, r"D:\acter\settings");
    }

    /// **The version is the build's, and it is said in words** (decision 5). The raw
    /// identifier is what a bug report carries; nothing tries to spell a commit out loud.
    #[test]
    fn a_release_says_its_number_and_a_development_build_says_it_is_one() {
        let released = about_a(Standing::Installed, Version::released("1.0.0"));
        assert_eq!(released.version, "1.0.0");
        assert_eq!(released.version_said, "Version 1.0.0.");

        let development = about_a(Standing::Development, Version::development("521c956"));
        assert_eq!(development.version, "development-521c956");
        assert_eq!(
            development.version_said,
            "Development build, commit 521c956."
        );
    }

    /// Every standing has a sentence of its own, and each is a whole one: this line is read
    /// aloud, and a listener who hears only a path has been told where without being told
    /// why.
    ///
    /// **The run of spaces is asserted because one got in** during an earlier attempt at
    /// this entry, from a line continuation a formatting pass unwrapped: the sentence still
    /// read correctly and carried eighteen spaces in the middle of it, which nothing in
    /// that version of this test could see.
    #[test]
    fn every_standing_is_a_finished_sentence_a_reader_can_speak() {
        let mut said = Vec::new();
        for standing in [
            Standing::Development,
            Standing::Portable,
            Standing::Installed,
            Standing::Directed,
            Standing::WhereItStarted,
        ] {
            let sentence = about_a(standing, Version::released("1.0.0")).settings_standing;
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(
                sentence.starts_with("Acter") || sentence.starts_with("This is"),
                "it says who it is about: {sentence}"
            );
            assert!(
                !sentence.contains("  "),
                "a sentence is spoken, so it carries no run of spaces: {sentence}"
            );
            said.push(sentence);
        }
        let mut distinct = said.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            said.len(),
            "two standings that read the same are one a listener cannot tell from the other"
        );
    }
}
