//! Adapter: `Settings` owns the settings folder, the document in it and the lock over that
//! document.

use std::fs::{copy, create_dir_all, read_to_string, rename};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use acter_core::{
    AcceptedHostKey, ConnectionStore, HostKeyOrigin, HostKeyStore, SavedConnection,
    StoredConnections, StoredSettings, same_name,
};
use tempfile::NamedTempFile;

use crate::container::{SettingsFolder, Standing, Version};

const SETTINGS: &str = "settings.json";

const PREVIOUS: &str = "settings-previous.json";

const UNREADABLE: &str = "settings-unreadable.json";

const EXPLAINED_SHELLS: &str = "explained_shells";

pub(crate) struct Settings {
    folder: SettingsFolder,
    version: Version,
    document: RwLock<StoredSettings>,
    /// `None` unless a document was read and would not parse.
    unreadable: Option<String>,
}

impl Settings {
    pub(crate) fn open(folder: SettingsFolder, version: Version) -> Self {
        let (document, unreadable) = read(&folder.path);
        Self {
            folder,
            version,
            document: RwLock::new(document),
            unreadable,
        }
    }

    pub(crate) fn folder(&self) -> &Path {
        &self.folder.path
    }

    pub(crate) fn standing(&self) -> Standing {
        self.folder.standing
    }

    pub(crate) fn version(&self) -> &Version {
        &self.version
    }

    pub(crate) fn explained_shells(&self) -> PathBuf {
        self.folder.path.join(EXPLAINED_SHELLS)
    }

    /// Holds the lock across the write, so two windows cannot race to rename into place;
    /// `Err` is a spoken sentence naming the folder.
    fn change(&self, what: &str, change: impl FnOnce(&mut StoredSettings)) -> Result<(), String> {
        let mut document = self.document.write().expect("the settings lock");
        let before = document.clone();
        change(&mut document);
        match write(&self.folder.path, &document) {
            Ok(()) => Ok(()),
            Err(_) => {
                *document = before;
                Err(self.could_not(what))
            }
        }
    }

    fn could_not(&self, what: &str) -> String {
        format!(
            "Could not {what}: the settings folder {} is not writable.",
            self.folder.path.display()
        )
    }
}

impl HostKeyStore for Settings {
    fn accepted(&self) -> Vec<AcceptedHostKey> {
        self.document
            .read()
            .expect("the settings lock")
            .host_keys
            .clone()
    }

    fn accept(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        fingerprint: &str,
    ) -> Result<(), String> {
        let accepted = AcceptedHostKey {
            host: host.to_owned(),
            port,
            algorithm: algorithm.to_owned(),
            fingerprint: fingerprint.to_owned(),
            accepted: today(SystemTime::now()),
            origin: HostKeyOrigin::Acter,
        };
        self.change("write down the server you accepted", |document| {
            let already = document
                .host_keys
                .iter()
                .any(|had| had.is_for(host, port) && had.fingerprint == accepted.fingerprint);
            if !already {
                document.host_keys.push(accepted);
            }
        })
    }
}

impl ConnectionStore for Settings {
    fn saved(&self) -> StoredConnections {
        let document = self.document.read().expect("the settings lock");
        StoredConnections {
            connections: document.connections.clone(),
            unreadable: self.unreadable.clone(),
        }
    }

    fn save(&self, connection: SavedConnection) -> Result<(), String> {
        self.change("save the connection", |document| {
            match document
                .connections
                .iter()
                .position(|saved| same_name(&saved.name, &connection.name))
            {
                Some(at) => document.connections[at] = connection,
                None => document.connections.push(connection),
            }
        })
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim().to_owned();
        self.change("rename the connection", |document| {
            if let Some(saved) = document
                .connections
                .iter_mut()
                .find(|saved| same_name(&saved.name, from))
            {
                saved.name = to;
            }
        })
    }

    fn forget(&self, name: &str) -> Result<(), String> {
        self.change("forget the connection", |document| {
            document
                .connections
                .retain(|saved| !same_name(&saved.name, name));
        })
    }

    fn offer_to_save(&self) -> bool {
        self.document
            .read()
            .expect("the settings lock")
            .offer_to_save
    }

    fn stop_offering_to_save(&self) -> Result<(), String> {
        self.change("record that preference", |document| {
            document.offer_to_save = false;
        })
    }
}

/// The UTC calendar day, as `YYYY-MM-DD`.
fn today(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil_from_days(i64::try_from(seconds / 86_400).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's `civil_from_days`, from a day number since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    })
    .unwrap_or(1);
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

fn read(folder: &Path) -> (StoredSettings, Option<String>) {
    let file = folder.join(SETTINGS);
    let Ok(text) = read_to_string(&file) else {
        return (StoredSettings::default(), None);
    };
    match serde_json::from_str::<StoredSettings>(&text) {
        Ok(document) => (document, None),
        Err(why) => {
            let moved = rename(&file, folder.join(UNREADABLE)).is_ok();
            (
                StoredSettings::default(),
                Some(unreadable(folder, &why.to_string(), moved)),
            )
        }
    }
}

fn unreadable(folder: &Path, why: &str, moved: bool) -> String {
    let folder = folder.display();
    if moved {
        format!(
            "Acter could not understand the connections it had saved, so it has started \
             with none. The file {SETTINGS} in the folder {folder} could not be read: \
             {why}. It has been moved to {UNREADABLE} in the same folder, so nothing in it \
             is lost."
        )
    } else {
        format!(
            "Acter could not understand the connections it had saved, so it has started \
             with none. The file {SETTINGS} in the folder {folder} could not be read: \
             {why}. Acter could not move it aside either, so it is still where it was."
        )
    }
}

fn write(folder: &Path, document: &StoredSettings) -> std::io::Result<()> {
    create_dir_all(folder)?;
    let target = folder.join(SETTINGS);
    // Copied, not renamed, so a failure before the persist below leaves the document in place.
    if target.exists() {
        copy(&target, folder.join(PREVIOUS))?;
    }
    // Same folder, because a rename is atomic only within one filesystem.
    let mut temporary = NamedTempFile::new_in(folder)?;
    serde_json::to_writer_pretty(&mut temporary, document)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.persist(&target).map_err(|failed| failed.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{read_to_string, write as write_file};

    use std::time::Duration;

    use acter_core::{LineOwner, SavedTarget, SetUp};
    use tempfile::TempDir;

    use super::*;

    fn settings(at: &TempDir) -> Settings {
        Settings::open(
            SettingsFolder {
                path: at.path().join("settings"),
                standing: Standing::Directed,
            },
            Version::development("521c956"),
        )
    }

    fn connection(name: &str) -> SavedConnection {
        SavedConnection {
            name: name.to_owned(),
            target: SavedTarget::Ssh {
                host: "example.org".to_owned(),
                port: 2222,
                account: "marlon".to_owned(),
            },
            set_up: SetUp::No,
            line_owner: LineOwner::Local,
        }
    }

    fn document(at: &TempDir) -> String {
        read_to_string(at.path().join("settings").join(SETTINGS)).expect("the document")
    }

    #[test]
    fn a_machine_that_has_never_saved_anything_has_no_folder_and_says_nothing() {
        let at = TempDir::new().expect("a directory for the test");

        let settings = settings(&at);

        assert_eq!(settings.saved(), StoredConnections::default());
        assert!(
            !at.path().join("settings").exists(),
            "the folder appears on the first write, not at startup"
        );
    }

    #[test]
    fn a_saved_connection_survives_being_written_and_read_by_a_second_acter() {
        let at = TempDir::new().expect("a directory for the test");

        settings(&at)
            .save(connection("work laptop"))
            .expect("it saves");

        let reopened = settings(&at);
        assert_eq!(
            reopened.saved().connections,
            vec![connection("work laptop")]
        );
        assert_eq!(reopened.saved().unreadable, None);
    }

    #[test]
    fn each_write_leaves_the_document_it_replaced_beside_it() {
        let at = TempDir::new().expect("a directory for the test");
        let settings = settings(&at);

        settings.save(connection("first")).expect("it saves");
        settings.save(connection("second")).expect("and again");

        let previous = read_to_string(at.path().join("settings").join(PREVIOUS))
            .expect("the document it replaced is still there");
        assert!(previous.contains("first"), "{previous}");
        assert!(
            !previous.contains("second"),
            "the previous copy is the one before this write: {previous}"
        );
        assert!(document(&at).contains("second"));
    }

    #[test]
    fn a_document_that_will_not_parse_is_moved_aside_and_named_in_a_sentence() {
        let at = TempDir::new().expect("a directory for the test");
        let folder = at.path().join("settings");
        create_dir_all(&folder).expect("a folder");
        write_file(folder.join(SETTINGS), "{ this is not json").expect("a damaged document");

        let settings = settings(&at);
        let answer = settings.saved();

        assert!(answer.connections.is_empty(), "Acter starts with nothing");
        let said = answer.unreadable.expect("and says why");
        assert!(said.contains(SETTINGS), "it names the file: {said}");
        assert!(said.contains(UNREADABLE), "and where it went: {said}");
        assert!(
            said.contains(&folder.display().to_string()),
            "and the folder both are in: {said}"
        );
        assert!(said.ends_with('.'), "it is read aloud: {said}");
        assert!(!said.contains("  "), "with no run of spaces: {said}");
        assert!(
            folder.join(UNREADABLE).exists() && !folder.join(SETTINGS).exists(),
            "the damaged document is moved rather than left to be written over"
        );
    }

    #[test]
    fn the_first_save_after_a_corruption_leaves_the_evidence_alone() {
        let at = TempDir::new().expect("a directory for the test");
        let folder = at.path().join("settings");
        create_dir_all(&folder).expect("a folder");
        write_file(folder.join(SETTINGS), "{ this is not json").expect("a damaged document");
        let settings = settings(&at);

        settings.save(connection("work laptop")).expect("it saves");

        assert_eq!(
            read_to_string(folder.join(UNREADABLE)).expect("the damaged one is still there"),
            "{ this is not json"
        );
        assert!(document(&at).contains("work laptop"));
    }

    #[test]
    fn saving_over_a_name_that_differs_only_in_case_replaces_it() {
        let at = TempDir::new().expect("a directory for the test");
        let settings = settings(&at);

        settings.save(connection("work laptop")).expect("it saves");
        settings.save(connection("Work Laptop")).expect("and again");

        let saved = settings.saved().connections;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Work Laptop");
    }

    #[test]
    fn renaming_and_forgetting_reach_the_document_and_the_disk() {
        let at = TempDir::new().expect("a directory for the test");
        let settings = settings(&at);
        settings.save(connection("work laptop")).expect("it saves");

        settings
            .rename("WORK LAPTOP", " home ")
            .expect("it renames");
        assert_eq!(settings.saved().connections[0].name, "home");
        assert!(document(&at).contains("home"));

        settings.forget("home").expect("it forgets");
        assert!(settings.saved().connections.is_empty());
        assert!(!document(&at).contains("home"));
    }

    #[test]
    fn the_offer_is_on_until_somebody_says_otherwise_and_then_it_stays_off() {
        let at = TempDir::new().expect("a directory for the test");

        assert!(settings(&at).offer_to_save(), "nobody has said");
        settings(&at)
            .stop_offering_to_save()
            .expect("the box is ticked");

        assert!(
            !settings(&at).offer_to_save(),
            "and a second Acter reads what the first wrote"
        );
    }

    #[test]
    fn the_folder_the_records_and_the_version_all_come_from_the_one_object() {
        let at = TempDir::new().expect("a directory for the test");
        let folder = at.path().join("settings");
        let settings = settings(&at);

        assert_eq!(settings.folder(), folder);
        assert_eq!(settings.explained_shells(), folder.join(EXPLAINED_SHELLS));
        assert_eq!(settings.standing(), Standing::Directed);
        assert_eq!(settings.version().identifier, "development-521c956");
    }

    #[test]
    fn a_write_that_cannot_happen_is_a_sentence_that_names_the_folder() {
        let at = TempDir::new().expect("a directory for the test");
        write_file(at.path().join("settings"), "not a folder").expect("something in the way");
        let settings = settings(&at);

        let refused = settings
            .save(connection("work laptop"))
            .expect_err("there is nowhere to write");

        assert_eq!(
            refused,
            format!(
                "Could not save the connection: the settings folder {} is not writable.",
                at.path().join("settings").display()
            )
        );
        assert!(!refused.contains("  "), "it is read aloud: {refused}");
        assert!(
            settings.saved().connections.is_empty(),
            "nothing claims to be saved that is not"
        );
    }

    #[test]
    fn the_document_on_disk_is_something_a_person_can_open_and_read() {
        let at = TempDir::new().expect("a directory for the test");

        settings(&at)
            .save(connection("work laptop"))
            .expect("it saves");

        let written = document(&at);
        assert!(written.contains("\"format\": 1"), "{written}");
        assert!(written.contains("\"work laptop\""), "{written}");
        assert!(written.ends_with('\n'), "a text file ends in a newline");
        assert!(!written.contains("password"), "{written}");
    }

    #[test]
    fn a_server_that_was_accepted_is_in_the_document_and_still_there_next_time() {
        let at = TempDir::new().expect("a directory for the test");

        settings(&at)
            .accept("example.org", 2222, "ssh-ed25519", "SHA256:abc")
            .expect("it is written down");

        let kept = settings(&at).accepted();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].is_for("example.org", 2222));
        assert_eq!(kept[0].algorithm, "ssh-ed25519");
        assert_eq!(kept[0].fingerprint, "SHA256:abc");
        assert_eq!(kept[0].accepted.len(), "2026-09-12".len(), "a day, stamped");
        assert!(kept[0].is_acters_own(), "and it is Acter\'s to keep");
        assert!(document(&at).contains("host_keys"), "under its own key");
        assert!(
            !document(&at).contains("origin"),
            "where a record came from is not something the document holds"
        );
    }

    #[test]
    fn accepting_the_same_server_twice_writes_one_record() {
        let at = TempDir::new().expect("a directory for the test");
        let settings = settings(&at);

        settings
            .accept("example.org", 22, "ssh-ed25519", "SHA256:abc")
            .expect("it is written down");
        settings
            .accept("example.org", 22, "ssh-ed25519", "SHA256:abc")
            .expect("and again");

        assert_eq!(settings.accepted().len(), 1);
    }

    #[test]
    fn a_server_that_changed_its_key_keeps_both_records() {
        let at = TempDir::new().expect("a directory for the test");
        let settings = settings(&at);

        settings
            .accept("example.org", 22, "ssh-ed25519", "SHA256:old")
            .expect("it is written down");
        settings
            .accept("example.org", 22, "ssh-ed25519", "SHA256:new")
            .expect("and the new one");

        let kept = settings.accepted();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].fingerprint, "SHA256:old");
        assert_eq!(kept[1].fingerprint, "SHA256:new");
    }

    #[test]
    fn a_server_that_cannot_be_written_down_says_so_and_names_the_folder() {
        let at = TempDir::new().expect("a directory for the test");
        write_file(at.path().join("settings"), "not a folder").expect("something in the way");

        let refused = settings(&at)
            .accept("example.org", 22, "ssh-ed25519", "SHA256:abc")
            .expect_err("there is nowhere to write");

        assert!(
            refused.starts_with("Could not write down the server you accepted:"),
            "{refused}"
        );
        assert!(refused.ends_with("is not writable."), "{refused}");
        assert!(!refused.contains("  "), "it is read aloud: {refused}");
    }

    #[test]
    fn the_day_a_record_is_stamped_with_is_the_calendar_day() {
        let at = |seconds: u64| today(UNIX_EPOCH + Duration::from_secs(seconds));

        assert_eq!(at(0), "1970-01-01", "the epoch itself");
        assert_eq!(at(86_399), "1970-01-01", "a second before it turns over");
        assert_eq!(at(86_400), "1970-01-02");
        assert_eq!(at(1_709_164_800), "2024-02-29");
        assert_eq!(at(1_709_251_200), "2024-03-01");
        assert_eq!(at(951_782_400), "2000-02-29");
    }
}
