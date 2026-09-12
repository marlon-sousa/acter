//! Adapter: `Settings` — the one object that owns the settings folder, the document in it
//! and the lock over that document (spec 26, decision 10).
//!
//! **It is built first in the composition root, before the connect service**, and
//! everything downstream is handed what it needs from it rather than resolving a path of
//! its own. The record of host keys and the record of explained shells take their folder
//! from here, and so does the connection store — which is this object, behind
//! [`ConnectionStore`].
//!
//! # Two halves, and they behave differently
//!
//! **The runtime values are set in the constructor and have getters and no setters**: the
//! folder with its standing, and the version. They are not in the document and are never
//! written.
//!
//! **The packaging is not among them**, and that is a deliberate narrowing of decision 10.
//! It is a build fact with exactly one consumer — the rule that decides where the folder
//! is — and by the time this object exists that rule has already run and left its answer in
//! [`Standing`]. A getter nothing calls is a value nothing tests. **The document is behind a `std::sync::RwLock`**, matching the
//! idiom `ConnectService` already uses, with typed getters and setters and no way to reach
//! the lock from outside. Every setter takes the lock, changes the document, writes it, and
//! answers a whole spoken sentence when the write fails.
//!
//! Because both halves are one object there is one lock and one write path, and nothing has
//! to ask anything else to persist on its behalf.
//!
//! # What a write does, and why (decision 9)
//!
//! One file holds everything now, so B8's per-file failure has no meaning and its safeguard
//! is replaced by three rules that matter more.
//!
//! - **Every write is atomic.** The document goes to a temporary file in the same folder
//!   and is renamed over the target. A crash, a full disk or a killed process cannot leave a
//!   half-written document, which with one file would mean every saved connection at once.
//! - **The previous document is kept**, as `settings-previous.json`, so a bad write is
//!   recoverable by hand and a person can be told which file to rename.
//! - **A document that will not parse is moved aside at load**, to
//!   `settings-unreadable.json`, and Acter starts with nothing saved. Without this the first
//!   save after a corruption destroys the evidence silently.
//!
//! **The folder is created on the first write, not at startup** (decision 3), so a machine
//! Acter was only ever run on and never saved anything from has no folder.
//!
//! **No crate does this for us, and that was looked at** (decision 10). `rustbreak` comes
//! closest — one file, an internal lock, atomic saves — and has no JSON serializer;
//! `persister` has JSON and atomic writes and no lock at all; `tauri-plugin-store` is the
//! official one and saves with a plain `fs::write` over the target, which is the one failure
//! this document cannot afford. So this is `serde_json`, `tempfile` and the standard
//! library's lock, all of which are already compiled here.

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

/// The document itself.
const SETTINGS: &str = "settings.json";

/// The document as it was before the last successful write (decision 9).
const PREVIOUS: &str = "settings-previous.json";

/// Where a document that would not parse is put, so the first save after a corruption
/// cannot destroy the evidence.
const UNREADABLE: &str = "settings-unreadable.json";

/// The file listing the shells this person has said not to be asked about again, one name
/// per line (spec B9.5, decision 10).
///
/// **Deliberately not folded into the document** (decision 2). It is a set rather than a
/// scalar, it works, and moving it would be a format change with nothing user-visible to
/// show for it. It changes folder and nothing else.
const EXPLAINED_SHELLS: &str = "explained_shells";

/// Everything Acter writes, and everything a listener can be told about where it went.
pub(crate) struct Settings {
    /// Where the settings are, and how they came to be there.
    folder: SettingsFolder,
    /// What this build is, as a bug report carries it and as About says it.
    version: Version,
    /// The document, and nothing outside this module can reach the lock.
    document: RwLock<StoredSettings>,
    /// What went wrong with a document that would not parse, and `None` when nothing did
    /// — which includes an ordinary first run with no document at all.
    unreadable: Option<String>,
}

impl Settings {
    /// Read what is there, moving aside anything that will not parse.
    ///
    /// **A folder that does not exist yet, and a document that is not there, are the
    /// ordinary first run** and produce no error at all (decision 9). Nothing is created
    /// here: the folder appears on the first write.
    pub(crate) fn open(folder: SettingsFolder, version: Version) -> Self {
        let (document, unreadable) = read(&folder.path);
        Self {
            folder,
            version,
            document: RwLock::new(document),
            unreadable,
        }
    }

    /// Where everything Acter writes goes, as a path a user can read out and type into a
    /// file manager.
    pub(crate) fn folder(&self) -> &Path {
        &self.folder.path
    }

    /// How it came to be there, as a whole sentence About reads after the path.
    pub(crate) fn standing(&self) -> Standing {
        self.folder.standing
    }

    /// What this build is: the identifier a bug report carries, and the sentence About
    /// speaks (decision 5).
    pub(crate) fn version(&self) -> &Version {
        &self.version
    }

    /// The shells this person has said not to be asked about again (spec B9.5, decision 10).
    pub(crate) fn explained_shells(&self) -> PathBuf {
        self.folder.path.join(EXPLAINED_SHELLS)
    }

    /// Change the document and write it, or answer the sentence to say instead.
    ///
    /// **The lock is held across the write**, deliberately: two windows writing at once is
    /// two documents racing to be the last one renamed into place, and the whole reason
    /// there is one document is that two stores can disagree about whether a write
    /// happened.
    fn change(&self, what: &str, change: impl FnOnce(&mut StoredSettings)) -> Result<(), String> {
        let mut document = self.document.write().expect("the settings lock");
        let before = document.clone();
        change(&mut document);
        match write(&self.folder.path, &document) {
            Ok(()) => Ok(()),
            Err(_) => {
                // **What is in memory goes back to what is on disk.** A window whose save
                // failed and then showed the connection as saved would be telling a
                // listener something the folder does not agree with, and the next
                // successful write would make it true without anybody having asked.
                *document = before;
                Err(self.could_not(what))
            }
        }
    }

    /// What a listener is told when a write did not happen (decision 6).
    ///
    /// **Naming the folder is the whole point**: a listener with an unwritable Program
    /// Files cannot see a red squiggle, and "could not save" without a place is a sentence
    /// they can do nothing with.
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

    /// **Appended unless it is already there**, and the day is stamped here because
    /// reading a clock is the world and the world is the adapter's.
    ///
    /// A server that changed its key becomes a *second* record rather than an overwrite:
    /// what was accepted and when is the whole history, and losing the old fingerprint
    /// would lose the evidence of the change.
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
            // **Acter's own by construction**: this method takes the facts rather than a
            // record, so a native one read out of the user's file has no way in.
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

/// Today, as `2026-09-12`.
///
/// **Written here rather than taken from a crate.** What is wanted is one date in one
/// format, and the conversion from a day number to a calendar date is Howard Hinnant's
/// civil-from-days: a dozen lines that have been correct since 1582. A date library would
/// be a dependency to keep updated forever for this.
///
/// Pure over the moment it is given, so the arithmetic is asserted against days somebody
/// can check rather than against whatever today happens to be.
fn today(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil_from_days(i64::try_from(seconds / 86_400).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The calendar date a day number since 1970-01-01 falls on.
///
/// Hinnant's algorithm, which shifts the year to start in March so a leap day lands at the
/// end of it and the month lengths become one arithmetic sequence.
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

/// What is in the folder, and what to say when what was there could not be understood.
///
/// **Three outcomes and no fourth.** Nothing there is a first run and says nothing. A
/// document that parses is the document. A document that does not is moved aside, named,
/// and Acter starts with nothing saved — never written over, because the first save after a
/// corruption would otherwise destroy the evidence silently.
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

/// What the Connect dialog says where it would otherwise say the list is empty
/// (decisions 9 and 16).
///
/// **It names both files and says what was wrong with the first**, because there is nothing
/// saved either way and the difference is whether the person should go looking for a file.
/// Read aloud, so the file names are said as words and the folder is a path a listener can
/// type into a file manager.
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
        // The one case where the file is still where it was: it could not be moved either.
        // Saying so matters, because the advice above — go and look at the other file —
        // would send somebody after a file that is not there.
        format!(
            "Acter could not understand the connections it had saved, so it has started \
             with none. The file {SETTINGS} in the folder {folder} could not be read: \
             {why}. Acter could not move it aside either, so it is still where it was."
        )
    }
}

/// Write the document, atomically, keeping the one it replaces.
///
/// **The folder is made here**, which is what decision 3 asks for: Acter makes its folder
/// when it has something to put in it rather than at startup.
fn write(folder: &Path, document: &StoredSettings) -> std::io::Result<()> {
    create_dir_all(folder)?;
    let target = folder.join(SETTINGS);
    // **Copied rather than renamed**, so a failure between here and the rename below leaves
    // the document that is there untouched rather than leaving no document at all.
    if target.exists() {
        copy(&target, folder.join(PREVIOUS))?;
    }
    // In the same folder, because a rename is only atomic within one filesystem — a
    // temporary file in the system's temp directory could be on another volume, where the
    // rename becomes a copy and stops being the thing this is for.
    let mut temporary = NamedTempFile::new_in(folder)?;
    // Pretty, because this file is something a person is told the path of and may open.
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

    /// **A first run has no folder and no document, and that is not an error** (decision 9).
    /// Nothing is created until there is something to put in it (decision 3).
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

    /// The round trip the whole file exists for, and the folder made on the way.
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

    /// **The document it replaced is kept** (decision 9), so a bad write is recoverable by
    /// hand and a person can be told which file to rename.
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

    /// **A document that will not parse is moved aside, named, and never written over**
    /// (decision 9). Acter starts with nothing saved and says which two files to look at.
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

    /// And the first save afterwards writes a new document rather than destroying the
    /// evidence, which is the failure decision 9 exists to prevent.
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

    /// The name rule's other half, at the store: saving over a name that differs only in
    /// case replaces the row rather than making a second one nobody could tell apart.
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

    /// The preference of decision 19, which is a field in this document rather than a port
    /// or a file of its own.
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

    /// **The runtime values have getters and no setters** (decision 10): they are not in
    /// the document and are never written.
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

    /// **A write that fails is a sentence naming the folder** (decision 6), and what is in
    /// memory goes back to what is on disk rather than claiming a save that did not happen.
    ///
    /// The unwritable folder is a *file* standing where the folder would be, which is the
    /// one way to make `create_dir_all` fail identically on every platform this builds for.
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

    /// The document a person opening the file meets: readable JSON, one connection per
    /// entry, and no field a password could be in.
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

    /// **Acter's own record of host keys is in the document now** (decision 2, amended in
    /// implementation 2026-09-12): accepted here, and there for the next Acter that opens
    /// it.
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

    /// Accepting the same server twice does not grow the document, which is what a set of
    /// `known_hosts` lines got for free.
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

    /// A server that changed its key is a *second* record rather than an overwrite: what
    /// was accepted and when is the whole history, and losing the old fingerprint would
    /// lose the evidence of the change.
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

    /// A record that cannot be written is the same sentence shape every other write
    /// failure has: it names the folder and says which action did not happen.
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

    /// **The day, asserted against days somebody can check** rather than against whatever
    /// today happens to be: the epoch, the turn of a day, and the two leap-year rules.
    #[test]
    fn the_day_a_record_is_stamped_with_is_the_calendar_day() {
        let at = |seconds: u64| today(UNIX_EPOCH + Duration::from_secs(seconds));

        assert_eq!(at(0), "1970-01-01", "the epoch itself");
        assert_eq!(at(86_399), "1970-01-01", "a second before it turns over");
        assert_eq!(at(86_400), "1970-01-02");
        // 2024 was a leap year, so 29 February exists and 1 March follows it.
        assert_eq!(at(1_709_164_800), "2024-02-29");
        assert_eq!(at(1_709_251_200), "2024-03-01");
        // 2000 was a leap year and 1900 was not, which is the rule this algorithm exists
        // to get right.
        assert_eq!(at(951_782_400), "2000-02-29");
    }
}
