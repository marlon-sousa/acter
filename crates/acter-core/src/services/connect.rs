//! Service: `ConnectService` — which session this window is on, and how it becomes a
//! different one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    Chosen, ConnectApi, ConnectQuestions, Connectable, Connected, Connection, ConnectionKind,
    ConnectionStore, EventSink, KeyAck, KeyPress, LaunchRequest, LineOwner, ProfileId,
    ProgramAnswer, ProgramQuestion, SavedConnection, SavedConnections, SavedRow, SavedTarget,
    SessionApi, SessionFactory, SessionId, SetUp, ShellInstall, Signatures, Started, SubmitAck,
    ThisComputer, Variant, catalogue, no_such_connection, refused, same_name,
};

const DEFAULT_SSH_PORT: u16 = 22;

pub struct ConnectService {
    factory: Arc<dyn SessionFactory>,
    machine: Arc<dyn ThisComputer>,
    signatures: Arc<dyn Signatures>,
    scripted: Vec<String>,
    kinds: Vec<ConnectionKind>,
    store: Arc<dyn ConnectionStore>,
    /// The name `acter --connect <name>` asked for, or `None` for an ordinary launch.
    requested: Option<String>,
    /// Which session is live, or `None` for a window connected to nothing.
    current: Mutex<Option<Live>>,
    next: AtomicU32,
}

struct Live {
    id: SessionId,
    label: String,
    profile: ProfileId,
    set_up: SetUp,
    /// The saved connection this session was started from, or `None` for one nobody has
    /// named yet.
    origin: Option<String>,
    line_owner: LineOwner,
    note: Option<String>,
    limit_explained: bool,
    session: Arc<dyn SessionApi>,
}

impl ConnectService {
    pub fn new(
        factory: Arc<dyn SessionFactory>,
        machine: Arc<dyn ThisComputer>,
        signatures: Arc<dyn Signatures>,
        kinds: Vec<ConnectionKind>,
        scripted: Vec<String>,
        store: Arc<dyn ConnectionStore>,
    ) -> Self {
        Self {
            factory,
            machine,
            signatures,
            kinds,
            scripted,
            store,
            requested: None,
            current: Mutex::new(None),
            next: AtomicU32::new(1),
        }
    }

    #[must_use]
    pub fn asked_for(mut self, name: Option<String>) -> Self {
        self.requested = name;
        self
    }

    /// `None` also for a stale id, so a line typed for a replaced session never runs in the
    /// new one.
    fn live(&self, session: SessionId) -> Option<Arc<dyn SessionApi>> {
        let current = self.current.lock().expect("session lock poisoned");
        current
            .as_ref()
            .filter(|live| live.id == session)
            .map(|live| Arc::clone(&live.session))
    }

    fn install(&self, program: &str) -> Option<ShellInstall> {
        self.machine.installs(program).into_iter().next()
    }

    /// What this machine would start for this profile, resolved once, or the sentence to say
    /// instead.
    fn chosen(&self, id: &ProfileId) -> Result<Chosen, String> {
        let program = match id {
            ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            }
            | ProfileId::Distribution { .. } => {
                self.machine
                    .wsl_distributions()
                    .map_err(|why| why.to_string())?;
                Some(self.found(ConnectionKind::Wsl)?)
            }
            ProfileId::Shell {
                kind: kind @ ConnectionKind::PowerShell,
            } => Some(
                kind.editions()
                    .iter()
                    .find_map(|edition| self.install(edition.program()))
                    .map(|install| install.program)
                    .ok_or_else(|| kind.instructions().to_owned())?,
            ),
            ProfileId::Shell {
                kind: kind @ ConnectionKind::Terminal,
            } => {
                let offered = self.machine.login_shells();
                let chosen = offered
                    .iter()
                    .find(|shell| shell.default)
                    .or_else(|| offered.first())
                    .ok_or_else(|| kind.instructions().to_owned())?;
                Some(chosen.install.program.clone())
            }
            ProfileId::Shell { kind } => Some(self.found(*kind)?),
            ProfileId::Install { program, .. } => Some(PathBuf::from(program)),
            ProfileId::Ssh { host, user, .. } => {
                if host.trim().is_empty() {
                    return Err(
                        "Acter needs the name or address of the machine to connect to.".to_owned(),
                    );
                }
                if user.trim().is_empty() {
                    return Err("Acter needs the name of the account to sign in as.".to_owned());
                }
                None
            }
            // A name nothing resolves has no file to verify, so the spawn's own error is what
            // the user hears.
            ProfileId::Program { program } => {
                self.install(program.trim()).map(|install| install.program)
            }
            ProfileId::Scripted { .. } => None,
        };
        Ok(Chosen {
            profile: id.clone(),
            program,
        })
    }

    fn found(&self, kind: ConnectionKind) -> Result<PathBuf, String> {
        self.install(kind.program())
            .map(|install| install.program)
            .ok_or_else(|| kind.instructions().to_owned())
    }

    /// `Ok` carries the clause to say at connection: `None` for a Microsoft or Apple
    /// signature or no file at all. `Err` is the sentence when the user declined to start it.
    fn verified(
        &self,
        chosen: &Chosen,
        label: &str,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Option<String>, String> {
        let Some(program) = chosen.program.as_deref() else {
            return Ok(None);
        };
        let verdict = self.signatures.verdict(program);
        if verdict.settled() {
            return Ok(verdict.note());
        }
        let asked = ProgramQuestion {
            label: label.to_owned(),
            program: program.display().to_string(),
            verdict: verdict.clone(),
        };
        match questions.unverified(asked) {
            ProgramAnswer::Start => Ok(verdict.note()),
            ProgramAnswer::DoNotStart => {
                Err(format!("Acter did not start {label}. {}", verdict.said()))
            }
        }
    }

    fn wsl_row(&self, row: &Connection) -> Connectable {
        let id = ProfileId::Shell {
            kind: ConnectionKind::Wsl,
        };
        match self.machine.wsl_distributions() {
            Ok(names) => Connectable {
                id,
                label: row.label.clone(),
                available: true,
                instructions: None,
                variants: names
                    .into_iter()
                    .map(|name| Variant {
                        label: name.clone(),
                        id: ProfileId::Distribution { name },
                        available: true,
                        instructions: None,
                    })
                    .collect(),
            },
            Err(why) => Connectable {
                id,
                label: row.label.clone(),
                available: false,
                instructions: Some(why.to_string()),
                variants: Vec::new(),
            },
        }
    }

    fn powershell_row(&self, row: &Connection) -> Connectable {
        let mut variants: Vec<Variant> = Vec::new();
        for edition in ConnectionKind::PowerShell.editions() {
            let installs = self.machine.installs(edition.program());
            if installs.is_empty() {
                variants.push(Variant {
                    id: ProfileId::Shell { kind: *edition },
                    label: format!("{}{NOT_AVAILABLE}", edition.label()),
                    available: false,
                    instructions: Some(edition.instructions().to_owned()),
                });
                continue;
            }
            for (id, install) in tell_apart(*edition, &installs).into_iter().zip(&installs) {
                variants.push(Variant {
                    label: self.named(id.label(), &install.program),
                    id,
                    available: true,
                    instructions: None,
                });
            }
        }

        let first = variants.iter().find(|variant| variant.available);
        Connectable {
            id: first.map_or(
                ProfileId::Shell {
                    kind: ConnectionKind::PowerShell,
                },
                |variant| variant.id.clone(),
            ),
            label: row.label.clone(),
            available: first.is_some(),
            instructions: row.instructions().map(ToOwned::to_owned),
            variants,
        }
    }

    fn terminal_row(&self, row: &Connection) -> Connectable {
        let shells = self.machine.login_shells();
        let variants: Vec<Variant> = shells
            .iter()
            .map(|shell| {
                let mut label = shell.name();
                if shell.default {
                    label.push_str(DEFAULT);
                }
                Variant {
                    label: self.named(label, shell.program()),
                    id: ProfileId::Install {
                        kind: ConnectionKind::Terminal,
                        program: shell.program().display().to_string(),
                        provenance: Some(shell.name()),
                    },
                    available: true,
                    instructions: None,
                }
            })
            .collect();

        let default = shells
            .iter()
            .position(|shell| shell.default)
            .unwrap_or_default();
        Connectable {
            id: variants.get(default).map_or(
                ProfileId::Shell {
                    kind: ConnectionKind::Terminal,
                },
                |variant| variant.id.clone(),
            ),
            label: row.label.clone(),
            available: !variants.is_empty(),
            instructions: row.instructions().map(ToOwned::to_owned),
            variants,
        }
    }

    fn named(&self, label: String, program: &Path) -> String {
        match self.signatures.known(program) {
            Some(verdict) if !verdict.settled() => format!("{label}{NOT_VERIFIED}"),
            _ => label,
        }
    }
}

/// Two installs of one program never share a directory, so the directory always tells them
/// apart.
fn tell_apart(edition: ConnectionKind, installs: &[ShellInstall]) -> Vec<ProfileId> {
    let single = installs.len() == 1;
    let said: Vec<Option<String>> = installs
        .iter()
        .map(|install| if single { None } else { install.qualifier() })
        .collect();
    let unique: Vec<Option<String>> = said
        .iter()
        .enumerate()
        .map(|(index, provenance)| {
            let shared = said
                .iter()
                .enumerate()
                .any(|(other, another)| other != index && another == provenance);
            if shared {
                Some(installs[index].directory())
            } else {
                provenance.clone()
            }
        })
        .collect();

    installs
        .iter()
        .zip(unique)
        .map(|(install, provenance)| ProfileId::Install {
            kind: edition,
            program: install.program.display().to_string(),
            provenance,
        })
        .collect()
}

const NOT_AVAILABLE: &str = " (not available)";

const DEFAULT: &str = " (default)";

const NOT_VERIFIED: &str = " (not verified)";

impl ConnectApi for ConnectService {
    fn connectable(&self) -> Vec<Connectable> {
        let mut listed: Vec<Connectable> = catalogue(&self.kinds, |kind| match kind {
            ConnectionKind::Wsl => self.machine.wsl_distributions().is_ok(),
            ConnectionKind::PowerShell => ConnectionKind::PowerShell
                .editions()
                .iter()
                .any(|edition| !self.machine.installs(edition.program()).is_empty()),
            ConnectionKind::Ssh => true,
            ConnectionKind::Terminal => !self.machine.login_shells().is_empty(),
            other => !self.machine.installs(other.program()).is_empty(),
        })
        .iter()
        .map(|row| match row.kind {
            ConnectionKind::Wsl => self.wsl_row(row),
            ConnectionKind::PowerShell => self.powershell_row(row),
            ConnectionKind::Terminal => self.terminal_row(row),
            ConnectionKind::Ssh => Connectable {
                id: ProfileId::Ssh {
                    host: String::new(),
                    port: DEFAULT_SSH_PORT,
                    user: String::new(),
                },
                label: row.label.clone(),
                available: true,
                instructions: None,
                variants: Vec::new(),
            },
            kind => self.shell_row(row, kind),
        })
        .collect();
        listed.extend(self.scripted.iter().map(|name| {
            let id = ProfileId::Scripted {
                name: name.to_owned(),
            };
            Connectable {
                label: id.label(),
                id,
                available: true,
                instructions: None,
                variants: Vec::new(),
            }
        }));
        listed
    }

    fn use_profile(
        &self,
        id: &ProfileId,
        set_up: SetUp,
        origin: Option<&str>,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Connected, String> {
        let chosen = self.chosen(id)?;
        let label = id.label();
        let agreed = self.verified(&chosen, &label, questions)?;
        let Started {
            session,
            note,
            limit_explained,
        } = self.factory.open(&chosen, set_up, questions)?;

        let note = note.or(agreed);
        let line_owner = origin
            .and_then(|named| self.remembered(named))
            .map_or(LineOwner::FarEnd, |saved| saved.line_owner);
        let origin = origin.map(ToOwned::to_owned);
        let next = SessionId(self.next.fetch_add(1, Ordering::SeqCst));
        let previous = {
            let mut current = self.current.lock().expect("session lock poisoned");
            current.replace(Live {
                id: next,
                label: label.clone(),
                profile: id.clone(),
                set_up,
                origin: origin.clone(),
                line_owner,
                note: note.clone(),
                limit_explained,
                session,
            })
        };
        // Outside the lock: dropping the last handle ends the shell, which can take time.
        drop(previous);

        Ok(Connected {
            session: next,
            label,
            note,
            limit_explained,
            saved_as: origin,
            line_owner,
        })
    }

    fn connected(&self) -> Option<Connected> {
        let current = self.current.lock().expect("session lock poisoned");
        current.as_ref().map(|live| Connected {
            session: live.id,
            label: live.label.clone(),
            note: live.note.clone(),
            limit_explained: live.limit_explained,
            saved_as: live.origin.clone(),
            line_owner: live.line_owner,
        })
    }

    fn saved(&self) -> SavedConnections {
        let stored = self.store.saved();
        let mut rows: Vec<SavedRow> = stored
            .connections
            .iter()
            .map(|saved| self.row(saved))
            .collect();
        rows.sort_by_key(|row| row.name.to_lowercase());
        SavedConnections {
            rows,
            unreadable: stored.unreadable,
        }
    }

    fn save_connection(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if let Some(why) = refused(name) {
            return Err(why.to_owned());
        }
        let (profile, set_up, line_owner, origin) = {
            let current = self.current.lock().expect("session lock poisoned");
            let live = current.as_ref().ok_or_else(|| NOTHING_TO_SAVE.to_owned())?;
            (
                live.profile.clone(),
                live.set_up,
                live.line_owner,
                live.origin.clone(),
            )
        };
        let its_own = origin.as_deref().is_some_and(|had| same_name(had, name));
        if !its_own && self.remembered(name).is_some() {
            return Err(already_exists(name));
        }
        self.store.save(SavedConnection {
            name: name.to_owned(),
            target: SavedTarget::of(&profile)?,
            set_up,
            line_owner,
        })?;
        let mut current = self.current.lock().expect("session lock poisoned");
        if let Some(live) = current.as_mut() {
            live.origin = Some(name.to_owned());
        }
        Ok(format!("Saved as {name}."))
    }

    fn rename_connection(&self, from: &str, to: &str) -> Result<String, String> {
        let to = to.trim();
        if let Some(why) = refused(to) {
            return Err(why.to_owned());
        }
        let existing = self
            .remembered(from)
            .ok_or_else(|| no_such_connection(from))?;
        if !same_name(&existing.name, to) && self.remembered(to).is_some() {
            return Err(already_exists(to));
        }
        self.store.rename(from, to)?;
        let mut current = self.current.lock().expect("session lock poisoned");
        if let Some(live) = current.as_mut()
            && live
                .origin
                .as_deref()
                .is_some_and(|had| same_name(had, from))
        {
            live.origin = Some(to.to_owned());
        }
        Ok(format!("{} is now called {to}.", existing.name))
    }

    fn forget_connection(&self, name: &str) -> Result<String, String> {
        let existing = self
            .remembered(name)
            .ok_or_else(|| no_such_connection(name))?;
        self.store.forget(name)?;
        let mut current = self.current.lock().expect("session lock poisoned");
        if let Some(live) = current.as_mut()
            && live
                .origin
                .as_deref()
                .is_some_and(|had| same_name(had, name))
        {
            live.origin = None;
        }
        Ok(format!("{} is no longer saved.", existing.name))
    }

    fn offer_to_save(&self) -> bool {
        self.store.offer_to_save()
    }

    fn stop_offering_to_save(&self) -> Result<(), String> {
        self.store.stop_offering_to_save()
    }

    fn requested_at_launch(&self) -> Option<LaunchRequest> {
        let asked = self.requested.as_deref()?;
        Some(match self.remembered(asked) {
            Some(saved) => LaunchRequest::Connect { name: saved.name },
            None => LaunchRequest::unknown(asked),
        })
    }
}

const NOTHING_TO_SAVE: &str = "Nothing is connected, so there is nothing to save.";

fn already_exists(name: &str) -> String {
    format!(
        "A connection named {name} already exists. Choose another name, or forget that one \
         first."
    )
}

impl ConnectService {
    fn shell_row(&self, row: &Connection, kind: ConnectionKind) -> Connectable {
        match self.install(kind.program()) {
            Some(install) => {
                let id = ProfileId::Install {
                    kind,
                    program: install.program.display().to_string(),
                    provenance: None,
                };
                Connectable {
                    label: self.named(id.label(), &install.program),
                    id,
                    available: true,
                    instructions: None,
                    variants: Vec::new(),
                }
            }
            None => Connectable {
                id: ProfileId::Shell { kind },
                label: row.label.clone(),
                available: row.available,
                instructions: row.instructions().map(ToOwned::to_owned),
                variants: Vec::new(),
            },
        }
    }

    fn remembered(&self, name: &str) -> Option<SavedConnection> {
        self.store
            .saved()
            .connections
            .into_iter()
            .find(|saved| same_name(&saved.name, name))
    }

    fn row(&self, saved: &SavedConnection) -> SavedRow {
        let resolved = self.reachable(&saved.target);
        SavedRow {
            name: saved.name.clone(),
            summary: saved.target.summary(),
            set_up: saved.set_up,
            line_owner: saved.line_owner,
            id: match &resolved {
                Ok(id) => id.clone(),
                Err(_) => saved.target.profile(),
            },
            available: resolved.is_ok(),
            instructions: resolved.err(),
        }
    }

    fn reachable(&self, target: &SavedTarget) -> Result<ProfileId, String> {
        match target {
            SavedTarget::Cmd => self.installed(ConnectionKind::Cmd),
            SavedTarget::PowerShell {
                edition,
                provenance,
            } => self.edition(*edition, provenance.as_deref()),
            SavedTarget::Wsl { distribution } => self.distribution(distribution),
            SavedTarget::Terminal => match self.machine.login_shells().is_empty() {
                false => Ok(ProfileId::Shell {
                    kind: ConnectionKind::Terminal,
                }),
                true => Err(ConnectionKind::Terminal.instructions().to_owned()),
            },
            SavedTarget::Program { .. } | SavedTarget::Ssh { .. } => Ok(target.profile()),
            SavedTarget::Scripted { scenario } => {
                match self.scripted.iter().any(|named| named == scenario) {
                    true => Ok(target.profile()),
                    false => Err(only_in_development(scenario)),
                }
            }
        }
    }

    fn installed(&self, kind: ConnectionKind) -> Result<ProfileId, String> {
        self.install(kind.program())
            .map(|install| ProfileId::Install {
                kind,
                program: install.program.display().to_string(),
                provenance: None,
            })
            .ok_or_else(|| kind.instructions().to_owned())
    }

    fn edition(
        &self,
        edition: ConnectionKind,
        provenance: Option<&str>,
    ) -> Result<ProfileId, String> {
        let installs = self.machine.installs(edition.program());
        let ids = tell_apart(edition, &installs);
        ids.iter()
            .find(|id| match id {
                ProfileId::Install {
                    provenance: had, ..
                } => had.as_deref() == provenance,
                _ => false,
            })
            .or_else(|| ids.first())
            .cloned()
            .ok_or_else(|| edition.instructions().to_owned())
    }

    fn distribution(&self, named: &str) -> Result<ProfileId, String> {
        let installed = self
            .machine
            .wsl_distributions()
            .map_err(|why| why.to_string())?;
        match installed.iter().any(|had| had == named) {
            true => Ok(ProfileId::Distribution {
                name: named.to_owned(),
            }),
            false => Err(gone(named)),
        }
    }
}

fn gone(distribution: &str) -> String {
    format!(
        "The WSL distribution {distribution} is not installed on this computer any more. \
         Install it by running wsl --install -d {distribution} from an administrator Command \
         Prompt, or forget this connection."
    )
}

/// Must match the sentence the factory refuses a scripted session with in a release build;
/// see crates/acter-app/src/container.rs.
fn only_in_development(scenario: &str) -> String {
    format!("The scripted session {scenario} is only available in a development build of Acter.")
}

impl SessionApi for ConnectService {
    /// Does nothing when nothing is live; the sink is not kept, because the frontend attaches
    /// again after each `use_profile`.
    fn attach_session(&self, session: SessionId, sink: Arc<dyn EventSink>) {
        if let Some(live) = self.live(session) {
            live.attach_session(session, sink);
        }
    }

    fn submit_command(&self, session: SessionId, line: &str) -> SubmitAck {
        match self.live(session) {
            Some(live) => live.submit_command(session, line),
            None => SubmitAck::NotConnected,
        }
    }

    fn send_key(&self, session: SessionId, key: KeyPress) -> KeyAck {
        match self.live(session) {
            Some(live) => live.send_key(session, key),
            None => KeyAck::NothingToActOn,
        }
    }

    fn set_line_owner(&self, session: SessionId, owner: LineOwner) {
        let recorded = {
            let mut current = self.current.lock().expect("session lock poisoned");
            current
                .as_mut()
                .filter(|live| live.id == session)
                .map(|live| {
                    live.line_owner = owner;
                    Arc::clone(&live.session)
                })
        };
        if let Some(live) = recorded {
            live.set_line_owner(session, owner);
        }
    }

    fn paste(&self, session: SessionId, text: &str) {
        if let Some(live) = self.live(session) {
            live.paste(session, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{RememberedConnections, Unasked};

    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;

    use crate::{
        CommandId, Fault, HostKeyAnswer, HostKeyQuestion, LoginShell, NoDistributions,
        PasswordQuestion, PathStanding, Provenance, Secret, SessionEvent, SetupAnswer,
        SetupQuestion, Signer, SshQuestions, Verdict,
    };

    use super::*;
    use crate::offered;

    fn unasked() -> Arc<dyn ConnectQuestions> {
        Arc::new(Unasked)
    }

    struct FakeSession {
        alive: Arc<AtomicUsize>,
        submitted: Mutex<Vec<String>>,
        attached: Mutex<bool>,
        owner: Mutex<LineOwner>,
        pasted: Mutex<Vec<String>>,
    }

    impl FakeSession {
        fn new(alive: &Arc<AtomicUsize>) -> Arc<Self> {
            alive.fetch_add(1, Ordering::SeqCst);
            Arc::new(Self {
                alive: Arc::clone(alive),
                submitted: Mutex::new(Vec::new()),
                attached: Mutex::new(false),
                owner: Mutex::new(LineOwner::Local),
                pasted: Mutex::new(Vec::new()),
            })
        }

        fn was_attached(&self) -> bool {
            *self.attached.lock().unwrap()
        }

        fn owner(&self) -> LineOwner {
            *self.owner.lock().unwrap()
        }

        fn pasted(&self) -> Vec<String> {
            self.pasted.lock().unwrap().clone()
        }
    }

    impl Drop for FakeSession {
        fn drop(&mut self) {
            self.alive.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl SessionApi for FakeSession {
        fn attach_session(&self, _session: SessionId, _sink: Arc<dyn EventSink>) {
            *self.attached.lock().unwrap() = true;
        }
        fn submit_command(&self, _session: SessionId, line: &str) -> SubmitAck {
            self.submitted.lock().unwrap().push(line.to_owned());
            SubmitAck::Accepted {
                command_id: CommandId(1),
            }
        }
        fn send_key(&self, _session: SessionId, _key: KeyPress) -> KeyAck {
            KeyAck::Applied
        }
        fn set_line_owner(&self, _session: SessionId, owner: LineOwner) {
            *self.owner.lock().unwrap() = owner;
        }
        fn paste(&self, _session: SessionId, text: &str) {
            self.pasted.lock().unwrap().push(text.to_owned());
        }
    }

    #[derive(Default)]
    struct FakeFactory {
        alive: Arc<AtomicUsize>,
        opened: Mutex<Vec<Chosen>>,
        last: Mutex<Option<Arc<FakeSession>>>,
        refuses: Mutex<Option<(ProfileId, String)>>,
        set_up: Mutex<Vec<SetUp>>,
    }

    impl FakeFactory {
        fn refusing(profile: ProfileId, why: &str) -> Self {
            let factory = Self::default();
            *factory.refuses.lock().unwrap() = Some((profile, why.to_owned()));
            factory
        }
    }

    impl SessionFactory for FakeFactory {
        fn open(
            &self,
            chosen: &Chosen,
            set_up: SetUp,
            _questions: &Arc<dyn ConnectQuestions>,
        ) -> Result<Started, String> {
            self.set_up.lock().unwrap().push(set_up);
            if let Some((refused, why)) = self.refuses.lock().unwrap().as_ref()
                && *refused == chosen.profile
            {
                return Err(why.clone());
            }
            self.opened.lock().unwrap().push(chosen.clone());
            let session = FakeSession::new(&self.alive);
            *self.last.lock().unwrap() = Some(Arc::clone(&session));
            Ok(Started {
                session: session as Arc<dyn SessionApi>,
                note: None,
                limit_explained: false,
            })
        }
    }

    struct FakeMachine {
        programs: Vec<&'static str>,
        distributions: Result<Vec<String>, NoDistributions>,
        shells: Vec<&'static str>,
        mine: Option<&'static str>,
        extra: HashMap<&'static str, Vec<ShellInstall>>,
    }

    impl FakeMachine {
        fn complete() -> Self {
            Self {
                programs: vec!["cmd.exe", "powershell.exe", "pwsh.exe", "wsl.exe"],
                distributions: Ok(vec!["Ubuntu".to_owned(), "Debian".to_owned()]),
                shells: Vec::new(),
                mine: None,
                extra: HashMap::new(),
            }
        }

        fn a_mac() -> Self {
            Self {
                programs: Vec::new(),
                distributions: Err(NoDistributions::NotInstalled),
                shells: vec![
                    "/bin/bash",
                    "/bin/csh",
                    "/bin/dash",
                    "/bin/ksh",
                    "/bin/sh",
                    "/bin/tcsh",
                    "/bin/zsh",
                ],
                mine: Some("/bin/zsh"),
                extra: HashMap::new(),
            }
        }

        fn a_mac_with_no_shells() -> Self {
            Self {
                shells: Vec::new(),
                mine: None,
                ..Self::a_mac()
            }
        }

        fn without(program: &'static str) -> Self {
            let mut machine = Self::complete();
            machine.programs.retain(|have| *have != program);
            machine
        }

        fn without_wsl(reason: NoDistributions) -> Self {
            Self {
                distributions: Err(reason),
                ..Self::complete()
            }
        }

        fn with(mut self, program: &'static str, installs: Vec<ShellInstall>) -> Self {
            self.extra.insert(program, installs);
            self
        }
    }

    impl ThisComputer for FakeMachine {
        fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
            self.distributions.clone()
        }

        fn login_shells(&self) -> Vec<LoginShell> {
            let mine = self.mine;
            let ordered = mine.into_iter().chain(
                self.shells
                    .iter()
                    .copied()
                    .filter(|shell| Some(*shell) != mine),
            );
            ordered
                .map(|shell| LoginShell {
                    default: Some(shell) == mine,
                    install: ShellInstall {
                        program: PathBuf::from(shell),
                        provenance: Provenance::System,
                        standing: PathStanding::Absent,
                    },
                })
                .collect()
        }

        fn installs(&self, program: &str) -> Vec<ShellInstall> {
            if let Some(extra) = self.extra.get(program) {
                return extra.clone();
            }
            if !self.programs.contains(&program) {
                return Vec::new();
            }
            vec![ShellInstall {
                program: PathBuf::from(format!(r"C:\Windows\system32\{program}")),
                provenance: Provenance::System,
                standing: PathStanding::First,
            }]
        }

        fn login_shell(&self, _distribution: Option<&str>) -> Option<String> {
            None
        }
    }

    #[derive(Default)]
    struct FakeSignatures {
        verdicts: Mutex<Vec<(PathBuf, Verdict)>>,
        verified: Mutex<Vec<PathBuf>>,
        cached: Mutex<Vec<(PathBuf, Verdict)>>,
    }

    impl FakeSignatures {
        fn saying(program: &str, verdict: Verdict) -> Self {
            let signatures = Self::default();
            signatures
                .verdicts
                .lock()
                .unwrap()
                .push((PathBuf::from(program), verdict));
            signatures
        }
    }

    impl Signatures for FakeSignatures {
        fn verdict(&self, program: &Path) -> Verdict {
            self.verified.lock().unwrap().push(program.to_path_buf());
            self.verdicts
                .lock()
                .unwrap()
                .iter()
                .find(|(at, _)| at == program)
                .map_or(
                    Verdict::Trusted {
                        signer: Signer::Microsoft,
                    },
                    |(_, verdict)| verdict.clone(),
                )
        }

        fn known(&self, program: &Path) -> Option<Verdict> {
            self.cached
                .lock()
                .unwrap()
                .iter()
                .find(|(at, _)| at == program)
                .map(|(_, verdict)| verdict.clone())
        }
    }

    struct Agreeing;

    impl SshQuestions for Agreeing {
        fn host_key(&self, _question: HostKeyQuestion) -> HostKeyAnswer {
            HostKeyAnswer::Refuse
        }
        fn password(&self, _question: PasswordQuestion) -> Option<Secret> {
            None
        }
        fn tell(&self, _sentence: &str) {}
    }

    impl ConnectQuestions for Agreeing {
        fn unverified(&self, _question: ProgramQuestion) -> ProgramAnswer {
            ProgramAnswer::Start
        }
        fn set_up_session(&self, _question: SetupQuestion) -> SetupAnswer {
            SetupAnswer::Skip
        }
    }

    #[derive(Default)]
    struct Asking {
        asked: Mutex<Vec<ProgramQuestion>>,
    }

    impl SshQuestions for Asking {
        fn host_key(&self, _question: HostKeyQuestion) -> HostKeyAnswer {
            HostKeyAnswer::Refuse
        }
        fn password(&self, _question: PasswordQuestion) -> Option<Secret> {
            None
        }
        fn tell(&self, _sentence: &str) {}
    }

    impl ConnectQuestions for Asking {
        fn unverified(&self, question: ProgramQuestion) -> ProgramAnswer {
            self.asked.lock().unwrap().push(question);
            ProgramAnswer::DoNotStart
        }
        fn set_up_session(&self, _question: SetupQuestion) -> SetupAnswer {
            SetupAnswer::Skip
        }
    }

    fn service(machine: FakeMachine, scripted: &[&str]) -> (Arc<ConnectService>, Arc<FakeFactory>) {
        let (service, factory, _) = signed(machine, Arc::new(FakeSignatures::default()), scripted);
        (service, factory)
    }

    fn signed(
        machine: FakeMachine,
        signatures: Arc<FakeSignatures>,
        scripted: &[&str],
    ) -> (Arc<ConnectService>, Arc<FakeFactory>, Arc<FakeSignatures>) {
        let factory = Arc::new(FakeFactory::default());
        let service = ConnectService::new(
            Arc::clone(&factory) as Arc<dyn SessionFactory>,
            Arc::new(machine),
            Arc::clone(&signatures) as Arc<dyn Signatures>,
            offered("windows").to_vec(),
            scripted.iter().map(|name| (*name).to_owned()).collect(),
            Arc::new(RememberedConnections::default()),
        );
        (Arc::new(service), factory, signatures)
    }

    fn on_a_mac(
        machine: FakeMachine,
        signatures: Arc<FakeSignatures>,
    ) -> (Arc<ConnectService>, Arc<FakeFactory>, Arc<FakeSignatures>) {
        let factory = Arc::new(FakeFactory::default());
        let service = ConnectService::new(
            Arc::clone(&factory) as Arc<dyn SessionFactory>,
            Arc::new(machine),
            Arc::clone(&signatures) as Arc<dyn Signatures>,
            offered("macos").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );
        (Arc::new(service), factory, signatures)
    }

    fn row(listed: &[Connectable], kind: ConnectionKind) -> Connectable {
        listed
            .iter()
            .find(|row| match &row.id {
                ProfileId::Shell { kind: named } => *named == kind,
                ProfileId::Install { kind: named, .. } => *named == kind,
                ProfileId::Ssh { .. } => kind == ConnectionKind::Ssh,
                _ => false,
            })
            .expect("the kind is listed")
            .clone()
    }

    fn install(program: &str, provenance: Provenance, standing: PathStanding) -> ShellInstall {
        ShellInstall {
            program: PathBuf::from(program),
            provenance,
            standing,
        }
    }

    fn labels(listed: &[Connectable]) -> Vec<&str> {
        listed.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn ssh_is_always_offered_and_carries_no_machine_of_its_own() {
        let (service, _) = service(FakeMachine::without_wsl(NoDistributions::NotInstalled), &[]);

        let listed = service.connectable();
        let row = listed
            .iter()
            .find(|row| row.label == "SSH")
            .expect("SSH is offered even on a machine with nothing installed");

        assert!(row.available, "nothing has to be installed for SSH");
        assert_eq!(row.instructions, None, "so there is nothing to instruct");
        assert!(
            row.variants.is_empty(),
            "what to connect to is a form, not a list of variants"
        );
        assert_eq!(
            row.id,
            ProfileId::Ssh {
                host: String::new(),
                port: 22,
                user: String::new()
            },
            "the row itself names no machine"
        );
    }

    #[test]
    fn an_ssh_profile_that_names_no_machine_is_refused_with_a_sentence() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        for (profile, expected) in [
            (
                ProfileId::Ssh {
                    host: "   ".to_owned(),
                    port: 22,
                    user: "acter".to_owned(),
                },
                "machine",
            ),
            (
                ProfileId::Ssh {
                    host: "acter-ssh".to_owned(),
                    port: 22,
                    user: String::new(),
                },
                "account",
            ),
        ] {
            let Err(why) = service.use_profile(&profile, SetUp::Yes, None, &unasked()) else {
                panic!("an unfilled form does not connect");
            };
            assert!(why.contains(expected), "it says what is missing: {why}");
            assert!(why.ends_with('.'), "a spoken message ends: {why}");
        }
    }

    #[test]
    fn the_list_is_every_kind_this_machine_has_with_the_scripted_ones_last() {
        let (service, _) = service(FakeMachine::complete(), &["builtin", "unmarked"]);

        assert_eq!(
            labels(&service.connectable()),
            [
                "Command Prompt",
                "PowerShell",
                "WSL",
                "SSH",
                "Scripted: builtin",
                "Scripted: unmarked",
            ]
        );
        assert!(service.connectable().iter().all(|row| row.available));
    }

    #[test]
    fn wsl_carries_its_distributions_as_the_variants_the_panel_lists() {
        let (service, _) = service(FakeMachine::complete(), &[]);
        let listed = service.connectable();

        let wsl = listed
            .iter()
            .find(|row| row.label == "WSL")
            .expect("WSL is offered");
        assert_eq!(
            wsl.variants
                .iter()
                .map(|variant| variant.label.as_str())
                .collect::<Vec<_>>(),
            ["Ubuntu", "Debian"],
            "in the order WSL reports them"
        );
        assert_eq!(
            wsl.variants[0].id,
            ProfileId::Distribution {
                name: "Ubuntu".to_owned()
            }
        );
        assert!(
            listed
                .iter()
                .filter(|row| row.label == "Command Prompt")
                .all(|row| row.variants.is_empty()),
            "a kind that is one thing has nothing in its panel"
        );
    }

    #[test]
    fn an_edition_this_machine_lacks_is_still_listed_and_explains_itself() {
        let (service, _) = service(FakeMachine::without("pwsh.exe"), &[]);
        let listed = service.connectable();

        let powershell = listed
            .iter()
            .find(|row| row.label == "PowerShell")
            .expect("the kind is available, because one edition is");
        assert!(powershell.available);
        let missing = powershell
            .variants
            .iter()
            .find(|variant| variant.label.starts_with("PowerShell 7"))
            .expect("a missing edition is still offered");
        assert_eq!(missing.label, "PowerShell 7 (not available)");
        assert!(!missing.available);
        assert!(
            missing
                .instructions
                .as_deref()
                .expect("and says what to do about it")
                .contains("winget install"),
        );
        assert_eq!(
            powershell.id,
            ProfileId::Install {
                kind: ConnectionKind::WindowsPowerShell,
                program: r"C:\Windows\system32\powershell.exe".to_owned(),
                provenance: None,
            },
            "and choosing the row without opening the panel starts the edition that works,              as the file the list already resolved"
        );
    }

    #[test]
    fn a_kind_whose_every_edition_is_missing_is_listed_as_unavailable() {
        let mut machine = FakeMachine::complete();
        machine
            .programs
            .retain(|have| !have.contains("powershell") && !have.contains("pwsh"));
        let (service, _) = service(machine, &[]);
        let listed = service.connectable();

        let powershell = listed
            .iter()
            .find(|row| row.label.starts_with("PowerShell"))
            .expect("the kind is still offered");
        assert_eq!(powershell.label, "PowerShell (not available)");
        assert!(!powershell.available);
        assert!(powershell.instructions.is_some());
        assert_eq!(
            listed.last().map(|row| row.label.as_str()),
            Some("PowerShell (not available)"),
            "unavailable rows sort to the end"
        );
    }

    #[test]
    fn wsl_that_cannot_answer_is_one_row_carrying_wsls_own_reason() {
        for reason in [
            NoDistributions::NotInstalled,
            NoDistributions::NoneInstalled,
            NoDistributions::NotWorking {
                detail: "Please enable the Virtual Machine Platform feature.".to_owned(),
            },
        ] {
            let (service, _) = service(FakeMachine::without_wsl(reason.clone()), &[]);
            let listed = service.connectable();

            let wsl = listed
                .iter()
                .find(|row| row.label.starts_with("WSL"))
                .expect("WSL is offered even when it cannot be started");
            assert_eq!(wsl.label, "WSL (not available)");
            assert!(!wsl.available);
            assert_eq!(
                wsl.instructions.as_deref(),
                Some(reason.to_string().as_str())
            );
        }
    }

    #[test]
    fn a_build_that_offers_no_scripted_session_lists_none() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        assert!(
            !service
                .connectable()
                .iter()
                .any(|row| matches!(row.id, ProfileId::Scripted { .. })),
        );
    }

    #[test]
    fn a_mac_is_offered_a_terminal_row_carrying_its_own_shells() {
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        let listed = service.connectable();

        assert_eq!(
            listed
                .iter()
                .map(|row| row.label.clone())
                .collect::<Vec<_>>(),
            ["Terminal", "SSH"],
            "two kinds, in the order a listener meets them"
        );
        let terminal = row(&listed, ConnectionKind::Terminal);
        assert!(terminal.available);
        assert_eq!(
            terminal
                .variants
                .iter()
                .map(|variant| variant.label.clone())
                .collect::<Vec<_>>(),
            ["zsh (default)", "bash", "csh", "dash", "ksh", "sh", "tcsh",],
            "the account's own shell first and saying so, then the file's own order"
        );
    }

    #[test]
    fn the_row_itself_starts_the_shell_this_account_logs_in_to() {
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        let terminal = row(&service.connectable(), ConnectionKind::Terminal);

        assert_eq!(
            terminal.id,
            ProfileId::Install {
                kind: ConnectionKind::Terminal,
                program: "/bin/zsh".to_owned(),
                provenance: Some("zsh".to_owned()),
            },
            "the row is the default variant, not the first entry in the file"
        );
        assert_eq!(
            terminal.id, terminal.variants[0].id,
            "and it is the variant that says it is the default"
        );
    }

    #[test]
    fn a_shell_on_this_mac_is_never_a_row_of_its_own() {
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        let listed = service.connectable();

        assert_eq!(listed.len(), 2, "seven shells did not become seven rows");
        assert_eq!(row(&listed, ConnectionKind::Terminal).variants.len(), 7);
    }

    #[test]
    fn a_mac_is_not_offered_windows_shells_as_missing_ones() {
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        let listed = service.connectable();

        for absent in [
            ConnectionKind::Cmd,
            ConnectionKind::PowerShell,
            ConnectionKind::Wsl,
        ] {
            assert!(
                !listed.iter().any(|listed| match &listed.id {
                    ProfileId::Shell { kind } | ProfileId::Install { kind, .. } => *kind == absent,
                    _ => false,
                }),
                "{absent:?} is not something a Mac can be missing"
            );
        }
    }

    #[test]
    fn a_mac_with_nothing_to_log_in_to_says_so_and_keeps_its_row() {
        let (service, _, _) = on_a_mac(
            FakeMachine::a_mac_with_no_shells(),
            Arc::new(FakeSignatures::default()),
        );

        let listed = service.connectable();
        let terminal = row(&listed, ConnectionKind::Terminal);

        assert_eq!(listed.len(), 2, "the list is the same length either way");
        assert!(!terminal.available);
        assert!(terminal.variants.is_empty(), "nothing to enumerate");
        assert!(
            terminal
                .instructions
                .expect("an unavailable row explains itself")
                .contains("/etc/shells"),
            "and names the file to look at"
        );
    }

    #[test]
    fn a_shell_that_did_not_verify_last_time_says_so_in_its_name() {
        let signatures = Arc::new(FakeSignatures::default());
        signatures.cached.lock().unwrap().push((
            PathBuf::from("/bin/bash"),
            Verdict::Untrusted {
                fault: Fault::AdHoc,
            },
        ));
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), signatures);

        let terminal = row(&service.connectable(), ConnectionKind::Terminal);

        assert!(
            terminal
                .variants
                .iter()
                .any(|variant| variant.label == "bash (not verified)"),
            "the shell is named, then what is known about it: {:?}",
            terminal
                .variants
                .iter()
                .map(|variant| variant.label.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            terminal.variants.iter().all(|variant| variant.available),
            "and it is not a filter: the shell is still there to choose"
        );
    }

    #[test]
    fn drawing_a_macs_list_verifies_nothing() {
        let signatures = Arc::new(FakeSignatures::default());
        let (service, _, signatures) = on_a_mac(FakeMachine::a_mac(), signatures);

        service.connectable();

        assert!(
            signatures.verified.lock().unwrap().is_empty(),
            "a list that stalls on seven signature checks is a list that stalls"
        );
    }

    #[test]
    fn choosing_a_shell_starts_the_file_the_panel_named() {
        let (service, factory, _) =
            on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));
        let terminal = row(&service.connectable(), ConnectionKind::Terminal);
        let bash = terminal
            .variants
            .iter()
            .find(|variant| variant.label == "bash")
            .expect("bash is offered")
            .clone();

        service
            .use_profile(&bash.id, SetUp::Yes, None, &unasked())
            .expect("a shell this Mac has starts");

        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from("/bin/bash")),
            "the file, not the name"
        );
    }

    #[test]
    fn a_profile_naming_the_kind_alone_starts_the_accounts_own_shell() {
        let (service, factory, _) =
            on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Terminal,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("the account's own shell starts");

        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from("/bin/zsh"))
        );
    }

    #[test]
    fn a_mac_with_no_shells_refuses_with_the_sentence_the_list_would_have_read() {
        let (service, factory, _) = on_a_mac(
            FakeMachine::a_mac_with_no_shells(),
            Arc::new(FakeSignatures::default()),
        );

        let refused = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Terminal,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect_err("there is nothing to start");

        assert!(refused.contains("/etc/shells"), "{refused}");
        assert!(
            factory.opened.lock().unwrap().is_empty(),
            "and nothing was started"
        );
    }

    #[test]
    fn the_list_asks_the_machine_again_on_every_call() {
        struct Counting(AtomicUsize);
        impl ThisComputer for Counting {
            fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(NoDistributions::NotInstalled)
            }
            fn installs(&self, program: &str) -> Vec<ShellInstall> {
                vec![install(
                    &format!(r"C:\Windows\system32\{program}"),
                    Provenance::System,
                    PathStanding::First,
                )]
            }
            fn login_shells(&self) -> Vec<LoginShell> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            }
            fn login_shell(&self, _distribution: Option<&str>) -> Option<String> {
                unreachable!("building the list never asks a distribution what it runs")
            }
        }
        let machine = Arc::new(Counting(AtomicUsize::new(0)));
        let service = ConnectService::new(
            Arc::new(FakeFactory::default()),
            Arc::clone(&machine) as Arc<dyn ThisComputer>,
            Arc::new(FakeSignatures::default()),
            offered("windows").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );

        service.connectable();
        let after_one = machine.0.load(Ordering::SeqCst);
        service.connectable();

        assert!(
            machine.0.load(Ordering::SeqCst) > after_one,
            "a cached list is a list that is wrong without saying so"
        );
    }

    #[test]
    fn an_unconnected_window_refuses_a_line_and_has_nothing_to_act_on() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        assert_eq!(service.connected(), None);
        assert_eq!(
            service.submit_command(SessionId(1), "dir"),
            SubmitAck::NotConnected
        );
        assert_eq!(
            service.send_key(
                SessionId(1),
                KeyPress {
                    key: crate::Key::Char('c'),
                    ctrl: true,
                    shift: false,
                    alt: false,
                }
            ),
            KeyAck::NothingToActOn
        );
    }

    #[test]
    fn using_a_profile_makes_it_the_session_a_line_reaches() {
        let (service, factory) = service(FakeMachine::complete(), &[]);
        let id = ProfileId::Shell {
            kind: ConnectionKind::Cmd,
        };

        let connected = service
            .use_profile(&id, SetUp::Yes, None, &unasked())
            .expect("cmd starts");

        assert_eq!(connected.label, "Command Prompt");
        assert_eq!(service.connected(), Some(connected.clone()));
        assert_eq!(
            service.submit_command(connected.session, "dir"),
            SubmitAck::Accepted {
                command_id: CommandId(1)
            }
        );
        assert_eq!(
            *factory.opened.lock().unwrap(),
            [Chosen {
                profile: id,
                program: Some(PathBuf::from(r"C:\Windows\system32\cmd.exe")),
            }],
            "and the factory was handed the file rather than the name"
        );
    }

    #[test]
    fn replacing_a_session_lets_the_previous_one_go() {
        let (service, factory) = service(FakeMachine::complete(), &[]);

        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");
        assert_eq!(factory.alive.load(Ordering::SeqCst), 1);

        service
            .use_profile(
                &ProfileId::Distribution {
                    name: "Ubuntu".to_owned(),
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("Ubuntu starts");

        assert_eq!(
            factory.alive.load(Ordering::SeqCst),
            1,
            "connecting twice leaves one session, not two"
        );
        assert_eq!(
            service.connected().map(|c| c.label).as_deref(),
            Some("WSL: Ubuntu")
        );
    }

    #[test]
    fn a_line_for_the_replaced_session_is_refused_rather_than_run_in_the_new_one() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        let first = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");
        let second = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("PowerShell starts");

        assert_ne!(first.session, second.session);
        assert_eq!(
            service.submit_command(first.session, "rm -rf ."),
            SubmitAck::NotConnected
        );
        assert!(matches!(
            service.submit_command(second.session, "dir"),
            SubmitAck::Accepted { .. }
        ));
    }

    #[test]
    fn a_failed_use_leaves_the_running_session_working() {
        let refused = ProfileId::Shell {
            kind: ConnectionKind::WindowsPowerShell,
        };
        let factory = Arc::new(FakeFactory::refusing(
            refused.clone(),
            "Windows PowerShell could not be started: access is denied.",
        ));
        let service = ConnectService::new(
            Arc::clone(&factory) as Arc<dyn SessionFactory>,
            Arc::new(FakeMachine::complete()),
            Arc::new(FakeSignatures::default()),
            offered("windows").to_vec(),
            Vec::new(),
            Arc::new(RememberedConnections::default()),
        );
        let working = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");

        let why = service
            .use_profile(&refused, SetUp::Yes, None, &unasked())
            .expect_err("this one does not");

        assert_eq!(
            why,
            "Windows PowerShell could not be started: access is denied."
        );
        assert_eq!(service.connected(), Some(working.clone()));
        assert!(matches!(
            service.submit_command(working.session, "dir"),
            SubmitAck::Accepted { .. }
        ));
        assert_eq!(
            factory.alive.load(Ordering::SeqCst),
            1,
            "and the session they were in is still alive"
        );
    }

    #[test]
    fn using_something_this_machine_lacks_says_what_to_install() {
        let (service, factory) = service(FakeMachine::without("pwsh.exe"), &[]);

        let why = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::PowerShellSeven,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect_err("PowerShell 7 is not installed");

        assert_eq!(why, ConnectionKind::PowerShellSeven.instructions());
        assert!(
            factory.opened.lock().unwrap().is_empty(),
            "nothing is spawned to find out what the machine already answered"
        );
    }

    #[test]
    fn using_a_distribution_on_a_machine_without_wsl_says_wsls_own_reason() {
        let (service, _) = service(FakeMachine::without_wsl(NoDistributions::NotInstalled), &[]);

        let why = service
            .use_profile(
                &ProfileId::Distribution {
                    name: "Ubuntu".to_owned(),
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect_err("there is no WSL to start it in");

        assert_eq!(why, NoDistributions::NotInstalled.to_string());
    }

    #[test]
    fn attaching_to_nothing_is_quiet_and_attaching_to_something_arrives() {
        struct Nowhere;
        impl EventSink for Nowhere {
            fn send(&self, _event: SessionEvent) {}
        }
        let (service, factory) = service(FakeMachine::complete(), &[]);

        service.attach_session(SessionId(1), Arc::new(Nowhere));

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");
        service.attach_session(connected.session, Arc::new(Nowhere));

        let live = factory
            .last
            .lock()
            .unwrap()
            .clone()
            .expect("a session was made");
        assert!(
            live.was_attached(),
            "the sink reached the session that is live"
        );
    }

    #[test]
    fn handing_the_line_over_reaches_the_session_that_is_live() {
        let (service, factory) = service(FakeMachine::complete(), &[]);

        service.set_line_owner(SessionId(1), LineOwner::FarEnd);
        service.paste(SessionId(1), "nowhere");

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");
        service.set_line_owner(connected.session, LineOwner::FarEnd);
        service.paste(connected.session, "cargo test");

        let live = factory
            .last
            .lock()
            .unwrap()
            .clone()
            .expect("a session was made");
        assert_eq!(live.owner(), LineOwner::FarEnd);
        assert_eq!(live.pasted(), vec!["cargo test".to_owned()]);

        service.set_line_owner(SessionId(999), LineOwner::Local);
        assert_eq!(
            live.owner(),
            LineOwner::FarEnd,
            "a stale id must not change the session that is live"
        );
    }

    #[test]
    fn attaching_with_a_replaced_id_reaches_nothing() {
        struct Nowhere;
        impl EventSink for Nowhere {
            fn send(&self, _event: SessionEvent) {}
        }
        let (service, factory) = service(FakeMachine::complete(), &[]);

        let first = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");
        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("PowerShell starts");

        service.attach_session(first.session, Arc::new(Nowhere));

        let live = factory
            .last
            .lock()
            .unwrap()
            .clone()
            .expect("a session was made");
        assert!(!live.was_attached());
    }

    #[test]
    fn the_path_that_was_verified_is_the_path_that_is_started() {
        let (service, factory, signatures) = signed(
            FakeMachine::complete(),
            Arc::new(FakeSignatures::default()),
            &[],
        );

        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");

        let verified = signatures.verified.lock().unwrap().clone();
        let started: Vec<PathBuf> = factory
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter_map(|chosen| chosen.program.clone())
            .collect();

        assert_eq!(verified, [PathBuf::from(r"C:\Windows\system32\cmd.exe")]);
        assert_eq!(
            started, verified,
            "one resolution, checked and then started"
        );
    }

    #[test]
    fn building_the_list_verifies_nothing() {
        let (service, _, signatures) = signed(
            FakeMachine::complete(),
            Arc::new(FakeSignatures::default()),
            &["builtin"],
        );

        let listed = service.connectable();

        assert!(!listed.is_empty(), "there is a list to have built");
        assert!(
            signatures.verified.lock().unwrap().is_empty(),
            "nothing was checked to draw it"
        );
    }

    #[test]
    fn an_entry_that_already_failed_to_verify_says_so_in_its_name() {
        let signatures = Arc::new(FakeSignatures::default());
        signatures.cached.lock().unwrap().push((
            PathBuf::from(r"C:\Windows\system32\cmd.exe"),
            Verdict::Untrusted {
                fault: Fault::NotSigned,
            },
        ));
        let (service, _, signatures) = signed(FakeMachine::complete(), signatures, &[]);

        let listed = service.connectable();

        assert!(
            listed
                .iter()
                .any(|row| row.label == "Command Prompt (not verified)"),
            "it is named, not removed: {:?}",
            labels(&listed)
        );
        assert!(
            listed
                .iter()
                .find(|row| row.label.starts_with("Command Prompt"))
                .expect("it kept its place in the list")
                .available,
            "a verdict is not a filter (decision 6)"
        );
        assert!(signatures.verified.lock().unwrap().is_empty());
    }

    #[test]
    fn starting_a_file_that_did_not_verify_asks_first_and_the_default_starts_nothing() {
        let signatures = Arc::new(FakeSignatures::saying(
            r"C:\Windows\system32\cmd.exe",
            Verdict::Untrusted {
                fault: Fault::UntrustedRoot {
                    signer: Some("Contoso Corporation".to_owned()),
                },
            },
        ));
        let (service, factory, _) = signed(FakeMachine::complete(), signatures, &[]);
        let asking = Arc::new(Asking::default());
        let questions = Arc::clone(&asking) as Arc<dyn ConnectQuestions>;

        let why = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &questions,
            )
            .expect_err("saying nothing starts nothing");

        let asked = asking.asked.lock().unwrap();
        let question = asked.first().expect("the user was asked");
        assert_eq!(question.label, "Command Prompt");
        assert_eq!(question.program, r"C:\Windows\system32\cmd.exe");
        assert_eq!(
            question.verdict.signer().as_deref(),
            Some("Contoso Corporation")
        );
        assert!(
            factory.opened.lock().unwrap().is_empty(),
            "and nothing was spawned"
        );
        assert!(why.starts_with("Acter did not start Command Prompt."));
        assert!(
            why.contains("does not trust"),
            "the sentence a listener hears is the verdict's own: {why}"
        );
        assert_eq!(service.connected(), None, "and the window is where it was");
    }

    #[test]
    fn starting_it_anyway_starts_it_and_says_so_once() {
        let signatures = Arc::new(FakeSignatures::saying(
            r"C:\Windows\system32\cmd.exe",
            Verdict::Untrusted {
                fault: Fault::NotSigned,
            },
        ));
        let (service, factory, _) = signed(FakeMachine::complete(), signatures, &[]);
        let questions = Arc::new(Agreeing) as Arc<dyn ConnectQuestions>;

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &questions,
            )
            .expect("saying so starts it");

        assert_eq!(connected.label, "Command Prompt");
        assert_eq!(
            connected.note.as_deref(),
            Some("started although nothing has signed it")
        );
        assert_eq!(factory.opened.lock().unwrap().len(), 1);
    }

    #[test]
    fn connecting_to_a_normally_installed_shell_says_nothing_new() {
        let (service, _, _) = signed(
            FakeMachine::complete(),
            Arc::new(FakeSignatures::default()),
            &[],
        );

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &unasked(),
            )
            .expect("cmd starts");

        assert_eq!(connected.note, None);
    }

    #[test]
    fn a_shell_signed_by_somebody_else_starts_and_says_who() {
        let signatures = Arc::new(FakeSignatures::saying(
            r"C:\Windows\system32\cmd.exe",
            Verdict::Trusted {
                signer: Signer::Other {
                    name: "Contoso Corporation".to_owned(),
                },
            },
        ));
        let (service, _, _) = signed(FakeMachine::complete(), signatures, &[]);
        let asking = Arc::new(Asking::default());

        let connected = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                None,
                &(Arc::clone(&asking) as Arc<dyn ConnectQuestions>),
            )
            .expect("a file this machine trusts starts");

        assert_eq!(
            connected.note.as_deref(),
            Some("signed by Contoso Corporation")
        );
        assert!(
            asking.asked.lock().unwrap().is_empty(),
            "there is nothing for the user to decide"
        );
    }

    #[test]
    fn two_installs_of_one_edition_are_told_apart_by_where_they_came_from() {
        let machine = FakeMachine::complete().with(
            "pwsh.exe",
            vec![
                install(
                    r"C:\Program Files\PowerShell\7\pwsh.exe",
                    Provenance::Directory {
                        version: "7".to_owned(),
                        preview: false,
                    },
                    PathStanding::First,
                ),
                install(
                    r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe",
                    Provenance::Store {
                        family: "Microsoft.PowerShell_8wekyb3d8bbwe".to_owned(),
                        preview: false,
                    },
                    PathStanding::Named,
                ),
            ],
        );
        let (service, _) = service(machine, &[]);

        let listed = service.connectable();
        let powershell = listed
            .iter()
            .find(|row| row.label == "PowerShell")
            .expect("the kind is one row however many installs there are");

        assert_eq!(
            powershell
                .variants
                .iter()
                .map(|variant| variant.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Windows PowerShell",
                "PowerShell 7",
                "PowerShell 7 (Microsoft Store)",
            ],
            "and the one PATH resolves first keeps the plain name"
        );
        assert_eq!(
            powershell.variants[2].id,
            ProfileId::Install {
                kind: ConnectionKind::PowerShellSeven,
                program:
                    r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe"
                        .to_owned(),
                provenance: Some("Microsoft Store".to_owned()),
            }
        );
    }

    #[test]
    fn a_machine_with_one_powershell_seven_names_it_plainly() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        let listed = service.connectable();
        let powershell = listed
            .iter()
            .find(|row| row.label == "PowerShell")
            .expect("PowerShell is offered");

        assert_eq!(
            powershell
                .variants
                .iter()
                .map(|variant| variant.label.as_str())
                .collect::<Vec<_>>(),
            ["Windows PowerShell", "PowerShell 7"]
        );
    }

    #[test]
    fn an_install_that_says_nothing_is_told_apart_by_where_it_is() {
        let dotnet: PathBuf = ["Users", "someone", ".dotnet", "tools"].iter().collect();
        let tools: PathBuf = ["tools", "pwsh"].iter().collect();
        let machine = FakeMachine::complete().with(
            "pwsh.exe",
            vec![
                install(
                    &dotnet.join("pwsh.exe").display().to_string(),
                    Provenance::Indeterminable,
                    PathStanding::First,
                ),
                install(
                    &tools.join("pwsh.exe").display().to_string(),
                    Provenance::Indeterminable,
                    PathStanding::Named,
                ),
            ],
        );
        let (service, _) = service(machine, &[]);

        let listed = service.connectable();
        let powershell = listed
            .iter()
            .find(|row| row.label == "PowerShell")
            .expect("PowerShell is offered");

        assert_eq!(
            powershell
                .variants
                .iter()
                .map(|variant| variant.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Windows PowerShell".to_owned(),
                format!("PowerShell 7 ({})", dotnet.display()),
                format!("PowerShell 7 ({})", tools.display()),
            ]
        );
    }

    #[test]
    fn choosing_one_of_two_installs_starts_that_one() {
        let machine = FakeMachine::complete().with(
            "pwsh.exe",
            vec![
                install(
                    r"C:\Program Files\PowerShell\7\pwsh.exe",
                    Provenance::Directory {
                        version: "7".to_owned(),
                        preview: false,
                    },
                    PathStanding::First,
                ),
                install(
                    r"C:\tools\pwsh\pwsh.exe",
                    Provenance::Indeterminable,
                    PathStanding::Absent,
                ),
            ],
        );
        let (service, factory) = service(machine, &[]);
        let chosen = ProfileId::Install {
            kind: ConnectionKind::PowerShellSeven,
            program: r"C:\tools\pwsh\pwsh.exe".to_owned(),
            provenance: Some(r"C:\tools\pwsh".to_owned()),
        };

        let connected = service
            .use_profile(&chosen, SetUp::Yes, None, &unasked())
            .expect("the chosen install starts");

        assert_eq!(connected.label, r"PowerShell 7 (C:\tools\pwsh)");
        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from(r"C:\tools\pwsh\pwsh.exe"))
        );
    }

    mod the_connections_somebody_saved {
        use super::*;

        use crate::{LineOwner, SavedConnection, SavedTarget, StoredConnections};

        fn with_saved(
            machine: FakeMachine,
            saved: Vec<SavedConnection>,
        ) -> (Arc<ConnectService>, Arc<RememberedConnections>) {
            let store = Arc::new(RememberedConnections::holding(saved));
            let service = ConnectService::new(
                Arc::new(FakeFactory::default()),
                Arc::new(machine),
                Arc::new(FakeSignatures::default()),
                offered("windows").to_vec(),
                ["builtin".to_owned()].to_vec(),
                Arc::clone(&store) as Arc<dyn ConnectionStore>,
            );
            (Arc::new(service), store)
        }

        fn saved(name: &str, target: SavedTarget) -> SavedConnection {
            SavedConnection {
                name: name.to_owned(),
                target,
                set_up: SetUp::Yes,
                line_owner: LineOwner::FarEnd,
            }
        }

        fn names(service: &ConnectService) -> Vec<String> {
            service
                .saved()
                .rows
                .into_iter()
                .map(|row| row.name)
                .collect()
        }

        fn row(service: &ConnectService, name: &str) -> SavedRow {
            service
                .saved()
                .rows
                .into_iter()
                .find(|row| row.name == name)
                .expect("the row is listed")
        }

        #[test]
        fn the_names_come_back_alphabetically_and_without_case() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![
                    saved("work laptop", SavedTarget::Cmd),
                    saved("Ada", SavedTarget::Cmd),
                    saved("zoe", SavedTarget::Cmd),
                    saved("Bob", SavedTarget::Cmd),
                ],
            );

            assert_eq!(names(&service), ["Ada", "Bob", "work laptop", "zoe"]);
        }

        #[test]
        fn every_kind_comes_back_as_a_row_the_panel_can_be_loaded_from() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![
                    saved("prompt", SavedTarget::Cmd),
                    saved(
                        "seven",
                        SavedTarget::PowerShell {
                            edition: ConnectionKind::PowerShellSeven,
                            provenance: None,
                        },
                    ),
                    saved(
                        "linux",
                        SavedTarget::Wsl {
                            distribution: "Ubuntu".to_owned(),
                        },
                    ),
                    saved(
                        "far",
                        SavedTarget::Ssh {
                            host: "example.org".to_owned(),
                            port: 2222,
                            account: "marlon".to_owned(),
                        },
                    ),
                    saved(
                        "fake",
                        SavedTarget::Scripted {
                            scenario: "builtin".to_owned(),
                        },
                    ),
                ],
            );

            let listed = service.saved();
            assert!(
                listed.rows.iter().all(|row| row.available),
                "this machine has all of them: {listed:?}"
            );
            assert!(
                listed.rows.iter().all(|row| row.instructions.is_none()),
                "a row that can be started explains nothing"
            );
            assert_eq!(
                row(&service, "linux").id,
                ProfileId::Distribution {
                    name: "Ubuntu".to_owned()
                }
            );
            assert_eq!(
                row(&service, "far").summary,
                "SSH, marlon at example.org, port 2222"
            );
            assert_eq!(listed.unreadable, None, "nothing went wrong with the file");
        }

        #[test]
        fn a_saved_powershell_connection_finds_its_edition_wherever_it_lives_now() {
            let mut machine = FakeMachine::complete();
            machine.extra.insert(
                "pwsh.exe",
                vec![install(
                    r"D:\somewhere-else\pwsh.exe",
                    Provenance::System,
                    PathStanding::First,
                )],
            );
            let (service, _) = with_saved(
                machine,
                vec![saved(
                    "seven",
                    SavedTarget::PowerShell {
                        edition: ConnectionKind::PowerShellSeven,
                        provenance: None,
                    },
                )],
            );

            let row = row(&service, "seven");

            assert!(row.available);
            assert_eq!(
                row.id,
                ProfileId::Install {
                    kind: ConnectionKind::PowerShellSeven,
                    program: r"D:\somewhere-else\pwsh.exe".to_owned(),
                    provenance: None,
                }
            );
        }

        #[test]
        fn a_distribution_that_was_uninstalled_is_listed_and_says_what_to_do() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![saved(
                    "linux",
                    SavedTarget::Wsl {
                        distribution: "Fedora".to_owned(),
                    },
                )],
            );

            let row = row(&service, "linux");

            assert!(!row.available);
            let instructions = row.instructions.expect("it says what to do about it");
            assert!(instructions.contains("Fedora"), "{instructions}");
            assert!(instructions.ends_with('.'), "{instructions}");
            assert!(!instructions.contains("  "), "{instructions}");
            assert_eq!(
                row.summary, "WSL, Fedora",
                "and it still says what it was, so a listener knows which row this is"
            );
        }

        #[test]
        fn a_scripted_scenario_this_build_does_not_offer_is_listed_as_unavailable() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![saved(
                    "fake",
                    SavedTarget::Scripted {
                        scenario: "no-such-scenario".to_owned(),
                    },
                )],
            );

            let row = row(&service, "fake");

            assert!(!row.available);
            assert!(
                row.instructions
                    .expect("it says why")
                    .contains("development build")
            );
        }

        #[test]
        fn a_document_that_would_not_parse_reaches_the_dialog_as_a_sentence() {
            let store = Arc::new(RememberedConnections::unreadable(
                "Acter could not understand the connections it had saved.",
            ));
            let service = ConnectService::new(
                Arc::new(FakeFactory::default()),
                Arc::new(FakeMachine::complete()),
                Arc::new(FakeSignatures::default()),
                offered("windows").to_vec(),
                Vec::new(),
                store as Arc<dyn ConnectionStore>,
            );

            let listed = service.saved();

            assert!(listed.rows.is_empty());
            assert_eq!(
                listed.unreadable.as_deref(),
                Some("Acter could not understand the connections it had saved.")
            );
        }

        #[test]
        fn saving_writes_the_profile_the_set_up_choice_and_who_has_the_line_now() {
            let (service, store) = with_saved(FakeMachine::complete(), Vec::new());
            let connected = service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::No,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");
            service.set_line_owner(connected.session, LineOwner::Local);

            let said = service.save_connection(" work laptop ").expect("it saves");

            assert_eq!(said, "Saved as work laptop.", "the name is trimmed");
            let written = &store.saved().connections[0];
            assert_eq!(written.name, "work laptop");
            assert_eq!(written.target, SavedTarget::Cmd);
            assert_eq!(written.set_up, SetUp::No);
            assert_eq!(
                written.line_owner,
                LineOwner::Local,
                "whoever owns the line now, not whoever owned it when it opened"
            );
        }

        #[test]
        fn saving_makes_the_name_the_sessions_origin() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");

            service.save_connection("work laptop").expect("it saves");

            assert_eq!(
                service.connected().expect("still connected").saved_as,
                Some("work laptop".to_owned())
            );
        }

        #[test]
        fn saving_under_its_own_origin_replaces_rather_than_refusing() {
            let (service, store) = with_saved(
                FakeMachine::complete(),
                vec![saved("work", SavedTarget::Cmd)],
            );
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Terminal,
                    },
                    SetUp::Yes,
                    Some("work"),
                    &unasked(),
                )
                .ok();

            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    Some("work"),
                    &unasked(),
                )
                .expect("cmd starts");

            let said = service.save_connection("WORK").expect("it replaces");

            assert_eq!(said, "Saved as WORK.");
            assert_eq!(store.saved().connections.len(), 1, "one row, not two");
        }

        #[test]
        fn saving_over_somebody_elses_name_is_refused_in_a_sentence() {
            let (service, store) = with_saved(
                FakeMachine::complete(),
                vec![saved("work laptop", SavedTarget::Cmd)],
            );
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");

            let refused = service
                .save_connection("Work Laptop")
                .expect_err("that name is taken");

            assert_eq!(
                refused,
                "A connection named Work Laptop already exists. Choose another name, or \
                 forget that one first."
            );
            assert!(!refused.contains("  "), "it is read aloud: {refused}");
            assert_eq!(
                store.saved().connections.len(),
                1,
                "and nothing was written"
            );
        }

        #[test]
        fn a_name_the_rule_forbids_is_refused_before_anything_is_written() {
            let (service, store) = with_saved(FakeMachine::complete(), Vec::new());
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");

            for bad in ["work/laptop", "  ", "work|laptop"] {
                let refused = service.save_connection(bad).expect_err("it is refused");
                assert!(refused.ends_with('.'), "{refused}");
                assert!(!refused.contains("  "), "{refused}");
            }
            assert!(store.saved().connections.is_empty());
        }

        #[test]
        fn saving_with_no_session_behind_the_window_says_there_is_nothing_to_save() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());

            let refused = service
                .save_connection("work laptop")
                .expect_err("there is nothing to save");

            assert_eq!(
                refused,
                "Nothing is connected, so there is nothing to save."
            );
        }

        #[test]
        fn a_wsl_session_with_no_distribution_is_refused_rather_than_written_down() {
            let (service, store) = with_saved(FakeMachine::complete(), Vec::new());
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Wsl,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("wsl starts");

            let refused = service
                .save_connection("ubuntu")
                .expect_err("there is nothing here to start again");

            assert!(
                refused.contains("New connection"),
                "it says what to do: {refused}"
            );
            assert!(
                store.saved().connections.is_empty(),
                "and nothing was written down"
            );
        }

        #[test]
        fn a_wsl_session_that_named_a_distribution_saves_as_that_distribution() {
            let (service, store) = with_saved(FakeMachine::complete(), Vec::new());
            service
                .use_profile(
                    &ProfileId::Distribution {
                        name: "Ubuntu".to_owned(),
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("the distribution starts");

            service.save_connection("ubuntu").expect("it is saved");

            assert_eq!(
                store
                    .saved()
                    .connections
                    .first()
                    .map(|row| row.target.clone()),
                Some(SavedTarget::Wsl {
                    distribution: "Ubuntu".to_owned()
                })
            );
        }

        #[test]
        fn a_save_that_cannot_be_written_says_the_stores_sentence() {
            let store = Arc::new(RememberedConnections::refusing(
                "Could not save the connection: the settings folder D:\\acter is not writable.",
            ));
            let service = ConnectService::new(
                Arc::new(FakeFactory::default()),
                Arc::new(FakeMachine::complete()),
                Arc::new(FakeSignatures::default()),
                offered("windows").to_vec(),
                Vec::new(),
                Arc::clone(&store) as Arc<dyn ConnectionStore>,
            );
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");

            let refused = service
                .save_connection("work laptop")
                .expect_err("the folder is not writable");

            assert!(
                refused.contains("D:\\acter"),
                "it names the folder: {refused}"
            );
            assert_eq!(
                service.connected().expect("still connected").saved_as,
                None,
                "a save that did not happen does not become the session's origin"
            );
        }

        #[test]
        fn a_saved_choice_about_the_line_wins_over_the_default() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![SavedConnection {
                    line_owner: LineOwner::Local,
                    ..saved("quiet", SavedTarget::Cmd)
                }],
            );

            let connected = service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    Some("quiet"),
                    &unasked(),
                )
                .expect("cmd starts");

            assert_eq!(connected.line_owner, LineOwner::Local);
            assert_eq!(connected.saved_as, Some("quiet".to_owned()));
        }

        #[test]
        fn a_new_connection_opens_on_the_far_ends_line_and_has_no_origin() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());

            let connected = service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    None,
                    &unasked(),
                )
                .expect("cmd starts");

            assert_eq!(connected.line_owner, LineOwner::FarEnd);
            assert_eq!(connected.saved_as, None, "nobody has named it");
        }

        #[test]
        fn renaming_answers_a_sentence_and_takes_the_live_origin_with_it() {
            let (service, store) = with_saved(
                FakeMachine::complete(),
                vec![saved("work", SavedTarget::Cmd)],
            );
            service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    Some("work"),
                    &unasked(),
                )
                .expect("cmd starts");

            let said = service
                .rename_connection("work", " home ")
                .expect("it renames");

            assert_eq!(said, "work is now called home.");
            assert_eq!(store.saved().connections[0].name, "home");
            assert_eq!(
                service.connected().expect("still connected").saved_as,
                Some("home".to_owned())
            );
        }

        #[test]
        fn renaming_refuses_a_collision_and_an_illegal_name() {
            let (service, store) = with_saved(
                FakeMachine::complete(),
                vec![
                    saved("work", SavedTarget::Cmd),
                    saved("home", SavedTarget::Cmd),
                ],
            );

            let collision = service
                .rename_connection("work", "HOME")
                .expect_err("that name is taken");
            assert!(collision.starts_with("A connection named HOME already exists."));

            let illegal = service
                .rename_connection("work", "wo:rk")
                .expect_err("that name breaks the rule");
            assert!(illegal.starts_with("A name cannot contain"));

            let missing = service
                .rename_connection("nothing", "something")
                .expect_err("there is no such row");
            assert_eq!(missing, "There is no saved connection named nothing.");

            assert_eq!(names(&service), ["home", "work"], "and nothing moved");
            assert_eq!(store.saved().connections.len(), 2);
        }

        #[test]
        fn renaming_a_row_to_its_own_name_in_another_case_is_allowed() {
            let (service, _) = with_saved(
                FakeMachine::complete(),
                vec![saved("work", SavedTarget::Cmd)],
            );

            service
                .rename_connection("work", "Work")
                .expect("it renames");

            assert_eq!(names(&service), ["Work"]);
        }

        #[test]
        fn forgetting_removes_the_row_and_leaves_the_session_running() {
            let (service, store) = with_saved(
                FakeMachine::complete(),
                vec![saved("work", SavedTarget::Cmd)],
            );
            let connected = service
                .use_profile(
                    &ProfileId::Shell {
                        kind: ConnectionKind::Cmd,
                    },
                    SetUp::Yes,
                    Some("work"),
                    &unasked(),
                )
                .expect("cmd starts");

            let said = service.forget_connection("WORK").expect("it forgets");

            assert_eq!(said, "work is no longer saved.");
            assert!(store.saved().connections.is_empty());
            let still = service.connected().expect("the session is still there");
            assert_eq!(still.session, connected.session);
            assert_eq!(still.saved_as, None, "it is no longer saved under anything");
        }

        #[test]
        fn forgetting_a_row_that_is_not_there_says_so() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());

            assert_eq!(
                service.forget_connection("work"),
                Err("There is no saved connection named work.".to_owned())
            );
        }

        #[test]
        fn the_offer_is_on_until_the_checkbox_says_otherwise() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());

            assert!(service.offer_to_save(), "nobody has said");
            service.stop_offering_to_save().expect("the box is ticked");

            assert!(!service.offer_to_save());
        }

        #[test]
        fn a_launch_switch_naming_a_saved_connection_becomes_a_request_to_connect() {
            let store = Arc::new(RememberedConnections::holding(vec![saved(
                "Work Laptop",
                SavedTarget::Cmd,
            )]));
            let service = ConnectService::new(
                Arc::new(FakeFactory::default()),
                Arc::new(FakeMachine::complete()),
                Arc::new(FakeSignatures::default()),
                offered("windows").to_vec(),
                Vec::new(),
                store as Arc<dyn ConnectionStore>,
            )
            .asked_for(Some("work laptop".to_owned()));

            assert_eq!(
                service.requested_at_launch(),
                Some(LaunchRequest::Connect {
                    name: "Work Laptop".to_owned()
                }),
                "the document's spelling, so the frontend can find the row by name"
            );
            assert!(
                service.connected().is_none(),
                "nothing is started before there is a window to ask a password in"
            );
        }

        #[test]
        fn a_launch_switch_naming_nothing_saved_becomes_a_sentence() {
            let service = ConnectService::new(
                Arc::new(FakeFactory::default()),
                Arc::new(FakeMachine::complete()),
                Arc::new(FakeSignatures::default()),
                offered("windows").to_vec(),
                Vec::new(),
                Arc::new(RememberedConnections::default()) as Arc<dyn ConnectionStore>,
            )
            .asked_for(Some("wrok laptop".to_owned()));

            let LaunchRequest::Unknown { name, said } =
                service.requested_at_launch().expect("the switch was given")
            else {
                panic!("nothing is saved under that name");
            };

            assert_eq!(name, "wrok laptop", "the name is kept as it was typed");
            assert_eq!(said, "There is no saved connection named wrok laptop.");
        }

        #[test]
        fn an_ordinary_launch_carries_no_request() {
            let (service, _) = with_saved(FakeMachine::complete(), Vec::new());

            assert_eq!(service.requested_at_launch(), None);
        }

        #[test]
        fn the_list_is_read_afresh_rather_than_remembered() {
            let (service, store) = with_saved(FakeMachine::complete(), Vec::new());
            assert_eq!(
                service.saved(),
                SavedConnections {
                    rows: Vec::new(),
                    unreadable: None
                }
            );

            store
                .save(saved("written elsewhere", SavedTarget::Cmd))
                .expect("another window saved one");

            assert_eq!(names(&service), ["written elsewhere"]);
            assert_eq!(
                store.saved(),
                StoredConnections {
                    connections: vec![saved("written elsewhere", SavedTarget::Cmd)],
                    unreadable: None
                }
            );
        }
    }
}
