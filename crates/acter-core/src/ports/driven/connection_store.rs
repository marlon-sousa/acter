//! Port (driven): where the saved connections are kept, and the preference about offering to
//! save.

use crate::SavedConnection;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredConnections {
    pub connections: Vec<SavedConnection>,
    /// Whole sentences to say instead of an empty list; `None` when nothing went wrong,
    /// including a first run with no document.
    pub unreadable: Option<String>,
}

/// Every `Err` is a whole spoken sentence.
pub trait ConnectionStore: Send + Sync {
    fn saved(&self) -> StoredConnections;

    /// Replaces any connection saved under the same name, compared without case.
    fn save(&self, connection: SavedConnection) -> Result<(), String>;

    /// Nothing saved under `from` is `Ok` and changes nothing.
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;

    fn forget(&self, name: &str) -> Result<(), String>;

    fn offer_to_save(&self) -> bool;

    fn stop_offering_to_save(&self) -> Result<(), String>;
}

/// A store in memory, for tests.
#[derive(Debug, Default)]
pub struct RememberedConnections {
    kept: std::sync::Mutex<Remembered>,
}

#[derive(Debug, Default)]
struct Remembered {
    connections: Vec<SavedConnection>,
    unreadable: Option<String>,
    offer: Offering,
    refusing: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Offering {
    #[default]
    Ask,
    Stopped,
}

impl RememberedConnections {
    pub fn holding(connections: Vec<SavedConnection>) -> Self {
        Self {
            kept: std::sync::Mutex::new(Remembered {
                connections,
                ..Remembered::default()
            }),
        }
    }

    pub fn unreadable(said: &str) -> Self {
        Self {
            kept: std::sync::Mutex::new(Remembered {
                unreadable: Some(said.to_owned()),
                ..Remembered::default()
            }),
        }
    }

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
