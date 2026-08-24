//! Router: the facts the About dialog reads out.
//!
//! **Read from the build rather than typed a second time** (spec A7, decision 3): the
//! version is the one Cargo compiled, so releasing changes what the dialog says with no
//! second edit and no chance of the two disagreeing. The frontend owns the dialog; this
//! owns only the facts.

use serde::Serialize;

/// What About says, in the order it says it.
#[derive(Serialize)]
pub(crate) struct About {
    name: &'static str,
    version: &'static str,
    copyright: &'static str,
    licence: &'static str,
}

#[tauri::command]
pub(crate) fn about() -> About {
    About {
        name: "Acter",
        version: env!("CARGO_PKG_VERSION"),
        copyright: "© 2026 Marlon Brandão de Sousa",
        licence: "MIT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version is the build's, which is the whole point of the command existing.
    #[test]
    fn the_version_is_the_one_cargo_compiled() {
        assert_eq!(about().version, env!("CARGO_PKG_VERSION"));
        assert!(
            !about().version.is_empty(),
            "a dialog that reads an empty version is worse than one that reads none"
        );
    }

    #[test]
    fn the_four_facts_are_all_speakable() {
        let about = about();
        for fact in [about.name, about.version, about.copyright, about.licence] {
            assert!(!fact.trim().is_empty(), "every fact is said out loud");
        }
    }
}
