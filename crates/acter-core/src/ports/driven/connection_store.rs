//! Port (driven): where the saved connections are kept, and the one preference that lives
//! beside them (spec 26, decision 10).
//!
//! **The connect service depends on this and never sees the settings object**, which is
//! what keeps a service depending on the port it actually uses. The object behind it owns
//! the folder, the document and the lock, and answers a whole spoken sentence when a write
//! fails; what reaches the domain is four actions over saved connections and two over the
//! preference, all of them synchronous and dyn-compatible like every other port here.
//!
//! **A failed write is a sentence rather than an error type.** Every other refusal on this
//! seam already is one, for the reason CLAUDE.md gives: the words a listener hears are
//! decided in the domain or in the adapter that knows what went wrong, never assembled by
//! whoever caught the failure. Naming the folder is the whole point of decision 6 — a
//! listener with an unwritable Program Files cannot see a red squiggle.

use crate::SavedConnection;

/// Everything saved, and the sentence to say instead when the document could not be read.
///
/// **Both, rather than one or the other** (decision 9). A document that will not parse is
/// moved aside and Acter starts with nothing saved, which is indistinguishable from a first
/// run unless something says so — and the difference between the two is whether the person
/// should go looking for a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredConnections {
    /// What is in the document, in the order the document lists them.
    pub connections: Vec<SavedConnection>,
    /// What went wrong with the document that was there, as whole sentences naming both
    /// files and what was wrong with the first — and `None` when nothing did, which
    /// includes an ordinary first run with no document at all.
    pub unreadable: Option<String>,
}

/// The saved connections, and the preference about offering to save.
pub trait ConnectionStore: Send + Sync {
    /// Everything saved, and the sentence to say instead when the document was unreadable
    /// (decision 9).
    ///
    /// **Freshly read on every call**, for `connectable`'s reason (spec B7, decision 6): a
    /// connection saved in another window while this one was open is there the next time
    /// the list is asked for, without a restart.
    fn saved(&self) -> StoredConnections;

    /// Write this connection down, replacing any saved under the same name.
    ///
    /// **Same name means the same name without case** (decision 8), so saving over "Work
    /// Laptop" as "work laptop" replaces the row rather than making a second one a listener
    /// could not tell from it.
    fn save(&self, connection: SavedConnection) -> Result<(), String>;

    /// Rename a saved connection. Nothing saved under `from` is not an error here — the
    /// service refuses that with its own sentence before it ever reaches the store.
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;

    /// Remove one. The one thing in this product nobody can undo, which is why the dialog
    /// asks first (decision 15).
    fn forget(&self, name: &str) -> Result<(), String>;

    /// Whether a new connection coming up should offer to save itself (decision 19).
    fn offer_to_save(&self) -> bool;

    /// Record that it should not, which is the offer dialog's checkbox and the only thing
    /// that writes this. **The File menu's Save connection is unaffected**, which is what
    /// the paragraph beside the checkbox is there to say.
    fn stop_offering_to_save(&self) -> Result<(), String>;
}

/// A store in memory, for the service tests and for a window that has no folder to write
/// to.
///
/// **A fake rather than a mock**, per ARCHITECTURE: it behaves like the real one — same
/// case-insensitive replace, same ordering — so a test asserts what a user would meet
/// rather than which methods were called. What it does not do is touch a disk, which is
/// the whole reason the real one is an adapter.
#[derive(Debug, Default)]
pub struct RememberedConnections {
    kept: std::sync::Mutex<Remembered>,
}

/// What the fake holds, behind one lock so it behaves like the real object's one document.
#[derive(Debug, Default)]
struct Remembered {
    connections: Vec<SavedConnection>,
    unreadable: Option<String>,
    offer: Offering,
    /// The sentence every write answers with instead of writing, when a test is about what
    /// a listener hears when a write fails.
    refusing: Option<String>,
}

/// Whether the fake has been told about the preference, so "nobody has said" and "somebody
/// said no" are different states rather than one `bool`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Offering {
    #[default]
    Ask,
    Stopped,
}

impl RememberedConnections {
    /// A store holding these, as though a document had been read.
    pub fn holding(connections: Vec<SavedConnection>) -> Self {
        Self {
            kept: std::sync::Mutex::new(Remembered {
                connections,
                ..Remembered::default()
            }),
        }
    }

    /// A store whose document would not parse, so the sentence is what it has instead
    /// (decision 9).
    pub fn unreadable(said: &str) -> Self {
        Self {
            kept: std::sync::Mutex::new(Remembered {
                unreadable: Some(said.to_owned()),
                ..Remembered::default()
            }),
        }
    }

    /// A store that refuses every write with this sentence, which is what an unwritable
    /// settings folder looks like from the domain's side (decision 6).
    pub fn refusing(said: &str) -> Self {
        Self {
            kept: std::sync::Mutex::new(Remembered {
                refusing: Some(said.to_owned()),
                ..Remembered::default()
            }),
        }
    }

    fn writing(&self) -> Result<std::sync::MutexGuard<'_, Remembered>, String> {
        let kept = self.kept.lock().expect("the fake store's lock");
        match &kept.refusing {
            Some(said) => Err(said.clone()),
            None => Ok(kept),
        }
    }
}

impl ConnectionStore for RememberedConnections {
    fn saved(&self) -> StoredConnections {
        let kept = self.kept.lock().expect("the fake store's lock");
        StoredConnections {
            connections: kept.connections.clone(),
            unreadable: kept.unreadable.clone(),
        }
    }

    fn save(&self, connection: SavedConnection) -> Result<(), String> {
        let mut kept = self.writing()?;
        match kept
            .connections
            .iter()
            .position(|saved| crate::same_name(&saved.name, &connection.name))
        {
            Some(at) => kept.connections[at] = connection,
            None => kept.connections.push(connection),
        }
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let mut kept = self.writing()?;
        if let Some(saved) = kept
            .connections
            .iter_mut()
            .find(|saved| crate::same_name(&saved.name, from))
        {
            saved.name = to.trim().to_owned();
        }
        Ok(())
    }

    fn forget(&self, name: &str) -> Result<(), String> {
        let mut kept = self.writing()?;
        kept.connections
            .retain(|saved| !crate::same_name(&saved.name, name));
        Ok(())
    }

    fn offer_to_save(&self) -> bool {
        self.kept.lock().expect("the fake store's lock").offer == Offering::Ask
    }

    fn stop_offering_to_save(&self) -> Result<(), String> {
        let mut kept = self.writing()?;
        kept.offer = Offering::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineOwner, SavedTarget, SetUp};

    fn connection(name: &str) -> SavedConnection {
        SavedConnection {
            name: name.to_owned(),
            target: SavedTarget::Cmd,
            set_up: SetUp::Yes,
            line_owner: LineOwner::FarEnd,
        }
    }

    /// The fake behaves the way the document does, which is what makes a service test
    /// about the service rather than about the fake: saving the same name twice replaces.
    #[test]
    fn saving_over_a_name_replaces_it_rather_than_making_a_second_row() {
        let store = RememberedConnections::default();

        store.save(connection("work laptop")).expect("it saves");
        store
            .save(SavedConnection {
                target: SavedTarget::Terminal,
                ..connection("Work Laptop")
            })
            .expect("and saves again");

        let saved = store.saved().connections;
        assert_eq!(saved.len(), 1, "two rows a listener could not tell apart");
        assert_eq!(saved[0].target, SavedTarget::Terminal);
        assert_eq!(saved[0].name, "Work Laptop", "the spelling last written");
    }

    #[test]
    fn renaming_and_forgetting_reach_the_row_whatever_its_case() {
        let store = RememberedConnections::holding(vec![connection("Work Laptop")]);

        store.rename("work laptop", "home").expect("it renames");
        assert_eq!(store.saved().connections[0].name, "home");

        store.forget("HOME").expect("it forgets");
        assert!(store.saved().connections.is_empty());
    }

    /// A store that refuses answers the sentence rather than a code, and writes nothing.
    #[test]
    fn a_store_that_cannot_write_answers_a_sentence_and_changes_nothing() {
        let store = RememberedConnections::refusing(
            "Could not save the connection: the settings folder D:\\acter is not writable.",
        );

        let refused = store.save(connection("work laptop"));

        assert_eq!(
            refused,
            Err(
                "Could not save the connection: the settings folder D:\\acter is not writable."
                    .to_owned()
            )
        );
        assert!(store.saved().connections.is_empty());
    }

    /// Nothing saved and nothing wrong is an ordinary first run, and it says nothing.
    #[test]
    fn a_first_run_has_nothing_saved_and_nothing_to_report() {
        let store = RememberedConnections::default();

        assert_eq!(store.saved(), StoredConnections::default());
        assert!(store.offer_to_save(), "the offer is on until somebody says");
    }

    #[test]
    fn a_document_that_would_not_parse_carries_its_sentence_instead() {
        let store = RememberedConnections::unreadable("Acter could not read its settings.");

        let answer = store.saved();

        assert!(answer.connections.is_empty());
        assert_eq!(
            answer.unreadable.as_deref(),
            Some("Acter could not read its settings.")
        );
    }
}
