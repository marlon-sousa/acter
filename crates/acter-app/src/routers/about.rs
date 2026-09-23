//! Adapter: the About Tauri router, the facts the About dialog reads.

use serde::Serialize;
use tauri::State;

use crate::adapters::Settings;
use crate::container::AppState;

#[derive(Serialize)]
pub(crate) struct About {
    name: &'static str,
    version: String,
    version_said: String,
    copyright: &'static str,
    licence: &'static str,
    settings_folder: String,
    settings_standing: &'static str,
}

#[tauri::command]
pub(crate) fn about(state: State<'_, AppState>) -> About {
    facts(&state.settings)
}

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

    #[test]
    fn it_says_the_settings_folder_it_was_given() {
        let about = about_a(Standing::Portable, Version::released("1.0.0"));

        assert_eq!(about.settings_folder, r"D:\acter\settings");
    }

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
