//! Service: `ConnectService` — which session this window is on, and how it becomes a
//! different one.
//!
//! It coordinates three things for one named use case: the catalogue policy, which decides
//! what belongs in a connect list and in what order; [`InstalledShells`], which answers
//! what this particular machine has; and [`SessionFactory`], which turns a chosen profile
//! into a running session. It names no adapter, so the whole of connecting — what is
//! offered, what replacing means, what happens when it fails — is tested with a fake
//! factory and a fake machine: no process, no runtime, no Tauri.
//!
//! # Why it implements both ports
//!
//! It is a [`ConnectApi`], which is its use case, **and** a [`SessionApi`], which is the
//! consequence of it: it owns which session is current, so it is the only thing that can
//! answer either question honestly. A submitted line has to reach whichever session is
//! running *now*, and a window with none has to answer rather than swallow it — neither is
//! answerable by something that does not own the swap.
//!
//! The alternative was a holder shared between two objects, and it is worse in the way that
//! matters here: two things reading one `Option` is two places that can disagree about
//! whether there is a session, at the moment a user is typing into one.
//!
//! # The unconnected window is a state, not a session that does nothing
//!
//! DESIGN, decided 2026-08-23: nothing is spawned until a profile is used. Modelling
//! "unconnected" as a live session with a dead transport would make every other part of the
//! system reason about a far end that was never there — the boundary tracker, the pacing
//! policy and the actor would all be running over a shell nobody started. Here it is an
//! absent `Option`, and the two things a user can do to an empty window are answered
//! directly: a submitted line is refused, and a keystroke has nothing to act on.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    ConnectApi, Connectable, Connected, Connection, ConnectionKind, EventSink, InstalledShells,
    KeyAck, KeyPress, ProfileId, SessionApi, SessionFactory, SessionId, SshQuestions, Started,
    SubmitAck, Variant, catalogue,
};

/// The port every SSH server listens on unless somebody moved it, which is what the form
/// starts filled in with.
const DEFAULT_SSH_PORT: u16 = 22;

/// The one session, and everything needed to replace it.
pub struct ConnectService {
    factory: Arc<dyn SessionFactory>,
    machine: Arc<dyn InstalledShells>,
    /// The scripted far ends this build offers, in the order they are listed.
    ///
    /// **Supplied rather than known** (spec B7, decision 7). What a scripted session *is* —
    /// a transcript, a chunking, a decorator over either — is a transport composition, and
    /// the domain has no business holding four names for compositions it cannot build. The
    /// composition root passes them, and passes none at all in a release build, so the gate
    /// is where the construction is.
    scripted: Vec<String>,
    /// Which session is live, or `None` for a window connected to nothing.
    current: Mutex<Option<Live>>,
    /// The session-id counter. Starts at 1, so 0 never names a session.
    next: AtomicU32,
}

/// The session that is running, with the two facts about it nothing else holds.
struct Live {
    id: SessionId,
    label: String,
    /// What was said about this far end at connection, kept so a window asking what it is
    /// connected to gets the same answer it was given when it connected.
    note: Option<String>,
    session: Arc<dyn SessionApi>,
}

impl ConnectService {
    /// A window connected to nothing, which is what an ordinary launch now opens.
    pub fn new(
        factory: Arc<dyn SessionFactory>,
        machine: Arc<dyn InstalledShells>,
        scripted: Vec<String>,
    ) -> Self {
        Self {
            factory,
            machine,
            scripted,
            current: Mutex::new(None),
            next: AtomicU32::new(1),
        }
    }

    /// The live session, but only if the caller is asking about the one that is live.
    ///
    /// **A stale id is answered as "not connected" rather than redirected**, which is the
    /// whole reason [`Connected::session`] is minted per connection. A line submitted a
    /// moment before the user replaced their shell must not be run in the new one: they
    /// typed it for a working directory, an environment and a machine that this session is
    /// not.
    fn live(&self, session: SessionId) -> Option<Arc<dyn SessionApi>> {
        let current = self.current.lock().expect("session lock poisoned");
        current
            .as_ref()
            .filter(|live| live.id == session)
            .map(|live| Arc::clone(&live.session))
    }

    /// Whether this machine can start this profile at all, as the sentence to say if not.
    ///
    /// Asked *before* the factory, so a kind the catalogue already reported as missing is
    /// refused with the instructions the catalogue would have shown rather than with
    /// whatever a failed spawn happened to say.
    fn startable(&self, id: &ProfileId) -> Result<(), String> {
        match id {
            // WSL is available when it can name a distribution, and its three ways of not
            // being available are three different sentences (spec B5.3, decision 6) — all
            // of them better than the catalogue's generic one, which has to serve a machine
            // nobody has asked yet.
            ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            }
            | ProfileId::Distribution { .. } => self
                .machine
                .wsl_distributions()
                .map(|_| ())
                .map_err(|why| why.to_string()),
            // A kind that comes in editions is startable when any of them is, and what
            // starts is the first that can — the same answer the row's own id carries.
            ProfileId::Shell {
                kind: kind @ ConnectionKind::PowerShell,
            } => {
                if kind
                    .editions()
                    .iter()
                    .any(|edition| self.machine.is_available(edition.program()))
                {
                    Ok(())
                } else {
                    Err(kind.instructions().to_owned())
                }
            }
            ProfileId::Shell { kind } => {
                if self.machine.is_available(kind.program()) {
                    Ok(())
                } else {
                    Err(kind.instructions().to_owned())
                }
            }
            // **Nothing on this machine to check** — Acter speaks SSH itself — so the only
            // thing that can be wrong here is the form. An empty host is refused with a
            // sentence rather than handed to the transport, which would answer with a
            // network error about a name nobody typed.
            ProfileId::Ssh { host, user, .. } => {
                if host.trim().is_empty() {
                    Err("Acter needs the name or address of the machine to connect to.".to_owned())
                } else if user.trim().is_empty() {
                    Err("Acter needs the name of the account to sign in as.".to_owned())
                } else {
                    Ok(())
                }
            }
            // A program named directly is not in the catalogue and has no instructions to
            // offer, so the answer to a name that does not start is the factory's — which
            // is the transport's own sentence about the spawn that failed.
            ProfileId::Program { .. } | ProfileId::Scripted { .. } => Ok(()),
        }
    }

    /// The connect list's WSL row: one row for the kind, carrying the installed
    /// distributions as its variants, or one row saying why there are none.
    ///
    /// **The distributions are variants rather than rows of their own** (spec A8,
    /// decision 1). They were rows in B7, which had no panel to put them in; the connect
    /// dialog does, and a listener arrowing the kinds should meet "WSL" once rather than
    /// once per distribution installed on a machine they may not have set up themselves.
    ///
    /// A row that cannot be started carries no variants, because there is nothing to
    /// enumerate inside something that is not there — and the reason it gives is WSL's own,
    /// which tells a user who has never installed it apart from one whose install is broken
    /// (spec B5.3, decision 6).
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
                        // A distribution that is not installed cannot be enumerated, so
                        // every one that has a name to list is one that can be started.
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

    /// The connect list's PowerShell row: one row for the kind, carrying its editions as
    /// variants (spec A11).
    ///
    /// **A missing edition stays in the panel and says what to do about it**, which is the
    /// difference from WSL and the reason [`Variant`] carries availability at all. A machine
    /// with Windows PowerShell and no PowerShell 7 has the kind, and listing only what is
    /// installed would teach that listener that Acter does not support the edition they
    /// have read about — which is precisely B5.4's argument, one level down.
    ///
    /// The row is available when *any* edition is, and the id it carries is the first
    /// edition that can actually be started, so choosing the row without opening the panel
    /// starts something rather than failing.
    fn powershell_row(&self, row: &Connection) -> Connectable {
        let variants: Vec<Variant> = ConnectionKind::PowerShell
            .editions()
            .iter()
            .map(|edition| {
                let available = self.machine.is_available(edition.program());
                Variant {
                    id: ProfileId::Shell { kind: *edition },
                    label: if available {
                        edition.label().to_owned()
                    } else {
                        format!("{}{NOT_AVAILABLE}", edition.label())
                    },
                    available,
                    instructions: (!available).then(|| edition.instructions().to_owned()),
                }
            })
            .collect();

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
}

/// The suffix an unavailable variant carries, in its **name** rather than in a visual state,
/// for the reason [`catalogue`](crate::catalogue) gives for a row: a greyed-out entry that
/// looks different and reads the same is the failure this product exists to avoid.
const NOT_AVAILABLE: &str = " (not available)";

impl ConnectApi for ConnectService {
    /// The catalogue, asked of this machine, with WSL carrying its distributions and the
    /// scripted sessions appended.
    ///
    /// **The machine is asked here rather than remembered**, twice over: `wsl.exe` is run
    /// and every program is looked up on every call. That is the cost of a list that is
    /// true when a user opens it (decision 6).
    ///
    /// The scripted sessions go last, after everything real, because they are a developer's
    /// tools and a user arrowing this list should meet their own shells first.
    fn connectable(&self) -> Vec<Connectable> {
        let mut listed: Vec<Connectable> = catalogue(|kind| match kind {
            ConnectionKind::Wsl => self.machine.wsl_distributions().is_ok(),
            // A kind that comes in editions is available when any of them is: a machine with
            // PowerShell 7 and no Windows PowerShell still has PowerShell.
            ConnectionKind::PowerShell => ConnectionKind::PowerShell
                .editions()
                .iter()
                .any(|edition| self.machine.is_available(edition.program())),
            // **Never asked of the machine, because it is not on the machine.** Acter
            // speaks SSH itself (spec B9, decision 1), so there is no executable to look
            // for — and looking for one found nothing and offered every user
            // "SSH (not available)", which is the opposite of true.
            ConnectionKind::Ssh => true,
            other => self.machine.is_available(other.program()),
        })
        .iter()
        .map(|row| match row.kind {
            ConnectionKind::Wsl => self.wsl_row(row),
            ConnectionKind::PowerShell => self.powershell_row(row),
            ConnectionKind::Ssh => Connectable {
                // **The row itself connects to nothing**, and that is the difference
                // between a kind you choose and a kind you fill in: what to connect to is
                // four fields in the panel, and the dialog builds the profile from them
                // (spec A8, decision 1). An empty one arriving here is refused with a
                // sentence rather than dialled.
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
            kind => Connectable {
                id: ProfileId::Shell { kind },
                label: row.label.clone(),
                available: row.available,
                instructions: row.instructions().map(ToOwned::to_owned),
                variants: Vec::new(),
            },
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

    /// **The order of operations is the decision** (spec B7, decision 5): the machine is
    /// asked, the new session is built, and only then is the old one let go. A failure at
    /// either of the first two steps returns before anything has been replaced, so the
    /// session the user was in is still running and still attached.
    ///
    /// Letting go is what ends the outgoing shell. Dropping the last `Arc` drops the
    /// request channel the pump is selecting on, the pump breaks out of its loop, and the
    /// transport it owns is dropped with it — which for a local one kills the process. It
    /// happens *outside* the lock, so a shell taking its time to die does not hold up a
    /// window that is already on the next session.
    fn use_profile(
        &self,
        id: &ProfileId,
        questions: &Arc<dyn SshQuestions>,
    ) -> Result<Connected, String> {
        self.startable(id)?;
        let Started { session, note } = self.factory.open(id, questions)?;

        let label = id.label();
        let next = SessionId(self.next.fetch_add(1, Ordering::SeqCst));
        let previous = {
            let mut current = self.current.lock().expect("session lock poisoned");
            current.replace(Live {
                id: next,
                label: label.clone(),
                note: note.clone(),
                session,
            })
        };
        drop(previous);

        Ok(Connected {
            session: next,
            label,
            note,
        })
    }

    fn connected(&self) -> Option<Connected> {
        let current = self.current.lock().expect("session lock poisoned");
        current.as_ref().map(|live| Connected {
            session: live.id,
            label: live.label.clone(),
            note: live.note.clone(),
        })
    }
}

impl SessionApi for ConnectService {
    /// Attaches to the live session, and does nothing at all when there is none.
    ///
    /// The sink is deliberately *not* kept for a session that has not been started yet.
    /// The frontend attaches after each successful `use_profile`, at the moment it has
    /// cleared its buffer, and a sink held here would deliver the new shell's opening into
    /// a buffer still showing the old one's output (spec B7, decision 1). Nothing is lost
    /// by waiting: each session holds what it said until somebody attaches (spec A9).
    fn attach_session(&self, session: SessionId, sink: Arc<dyn EventSink>) {
        if let Some(live) = self.live(session) {
            live.attach_session(session, sink);
        }
    }

    /// **A line typed into an unconnected window is answered, never swallowed** (decision
    /// 3). Silence is indistinguishable from a shell that is thinking, and a user who
    /// cannot see an empty buffer has nothing else to go on.
    fn submit_command(&self, session: SessionId, line: &str) -> SubmitAck {
        match self.live(session) {
            Some(live) => live.submit_command(session, line),
            None => SubmitAck::NotConnected,
        }
    }

    /// `NothingToActOn` rather than an answer of its own, and it is the honest one: with no
    /// session there is nothing to interrupt and nothing to end, which is exactly what that
    /// ack means. A fourth variant saying "and also you are not connected" would be a
    /// second way to say what the window said when it opened and says again the moment
    /// anything is submitted.
    fn send_key(&self, session: SessionId, key: KeyPress) -> KeyAck {
        match self.live(session) {
            Some(live) => live.send_key(session, key),
            None => KeyAck::NothingToActOn,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Unasked;

    use std::sync::atomic::AtomicUsize;

    use crate::{CommandId, NoDistributions, SessionEvent};

    use super::*;

    /// Nobody to ask, for every test here whose subject is not the asking.
    fn unasked() -> Arc<dyn SshQuestions> {
        Arc::new(Unasked)
    }

    /// A session that runs nothing and records that it existed.
    ///
    /// The drop counter is the point of it: decision 4's teeth are that a replaced session
    /// is really let go, and the only way to assert that from outside is to watch the last
    /// handle go.
    struct FakeSession {
        alive: Arc<AtomicUsize>,
        submitted: Mutex<Vec<String>>,
        attached: Mutex<bool>,
    }

    impl FakeSession {
        fn new(alive: &Arc<AtomicUsize>) -> Arc<Self> {
            alive.fetch_add(1, Ordering::SeqCst);
            Arc::new(Self {
                alive: Arc::clone(alive),
                submitted: Mutex::new(Vec::new()),
                attached: Mutex::new(false),
            })
        }

        fn was_attached(&self) -> bool {
            *self.attached.lock().unwrap()
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
    }

    /// Records every session it made, so a test can hold the last one and watch the
    /// previous ones go.
    #[derive(Default)]
    struct FakeFactory {
        alive: Arc<AtomicUsize>,
        opened: Mutex<Vec<ProfileId>>,
        /// The last session handed out, so a test can ask it what reached it.
        last: Mutex<Option<Arc<FakeSession>>>,
        /// A profile the factory refuses, with the sentence it refuses it in.
        refuses: Mutex<Option<(ProfileId, String)>>,
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
            profile: &ProfileId,
            _questions: &Arc<dyn SshQuestions>,
        ) -> Result<Started, String> {
            if let Some((refused, why)) = self.refuses.lock().unwrap().as_ref()
                && refused == profile
            {
                return Err(why.clone());
            }
            self.opened.lock().unwrap().push(profile.clone());
            let session = FakeSession::new(&self.alive);
            *self.last.lock().unwrap() = Some(Arc::clone(&session));
            Ok(Started {
                session: session as Arc<dyn SessionApi>,
                note: None,
            })
        }
    }

    /// A machine with exactly what a test says it has.
    struct FakeMachine {
        programs: Vec<&'static str>,
        distributions: Result<Vec<String>, NoDistributions>,
    }

    impl FakeMachine {
        /// Everything installed, two distributions.
        fn complete() -> Self {
            Self {
                programs: vec!["cmd.exe", "powershell.exe", "pwsh.exe", "wsl.exe"],
                distributions: Ok(vec!["Ubuntu".to_owned(), "Debian".to_owned()]),
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
    }

    impl InstalledShells for FakeMachine {
        fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
            self.distributions.clone()
        }
        fn is_available(&self, program: &str) -> bool {
            self.programs.contains(&program)
        }
    }

    fn service(machine: FakeMachine, scripted: &[&str]) -> (Arc<ConnectService>, Arc<FakeFactory>) {
        let factory = Arc::new(FakeFactory::default());
        let service = ConnectService::new(
            Arc::clone(&factory) as Arc<dyn SessionFactory>,
            Arc::new(machine),
            scripted.iter().map(|name| (*name).to_owned()).collect(),
        );
        (Arc::new(service), factory)
    }

    fn labels(listed: &[Connectable]) -> Vec<&str> {
        listed.iter().map(|row| row.label.as_str()).collect()
    }

    /// The acceptance criterion, spelled out: cmd, both editions, WSL once, and the
    /// scripted sessions last.
    ///
    /// **SSH is offered on every machine**, because there is nothing to install: Acter
    /// speaks the protocol itself. The row connects to nothing until the dialog's form has
    /// been filled in, which is what makes it the one kind that is a form rather than a
    /// choice (spec A8, decision 1).
    #[cfg(windows)]
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

    /// **An unfilled form is refused with a sentence rather than dialled.** Handing an empty
    /// host to the transport would answer with a network error about a name nobody typed,
    /// which is a worse thing for a listener to be told than what was actually wrong.
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
            let Err(why) = service.use_profile(&profile, &unasked()) else {
                panic!("an unfilled form does not connect");
            };
            assert!(why.contains(expected), "it says what is missing: {why}");
            assert!(why.ends_with('.'), "a spoken message ends: {why}");
        }
    }

    /// **One row for WSL rather than one per distribution** since A8: the dialog has a
    /// panel to put them in, so a listener arrows the kinds and meets "WSL" once however
    /// many distributions this machine has.
    #[cfg(windows)]
    #[test]
    fn the_list_is_every_kind_this_machine_has_with_the_scripted_ones_last() {
        let (service, _) = service(FakeMachine::complete(), &["builtin", "unmarked"]);

        assert_eq!(
            labels(&service.connectable()),
            [
                "Command Prompt",
                "PowerShell",
                "WSL",
                // Last of the real kinds, because it is the one that needs a form filled
                // in rather than a choice made (spec A8, decision 1).
                "SSH",
                "Scripted: builtin",
                "Scripted: unmarked",
            ]
        );
        assert!(service.connectable().iter().all(|row| row.available));
    }

    /// The distributions are what the dialog's panel holds, named without repeating the
    /// kind — and each one carries the id that starts it, so choosing one in the panel is
    /// as complete an answer as choosing a row.
    #[cfg(windows)]
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

    /// **What the list is for, on a machine that is missing something.** The row stays, it
    /// says so in its name, it goes last among the real shells, and it carries what to do
    /// about it — which is B5.4's whole argument, now asked of a real machine.
    /// **B5.4's argument, one level down** (spec A11). PowerShell 7 is an edition rather
    /// than a row now, and a machine without it must still be told it exists — listing only
    /// what is installed teaches a listener that Acter does not support the thing they have
    /// read about.
    #[cfg(windows)]
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
            ProfileId::Shell {
                kind: ConnectionKind::WindowsPowerShell
            },
            "and choosing the row without opening the panel starts the edition that works"
        );
    }

    /// A machine with neither edition has the kind, unavailable, saying so in its name — the
    /// row-level rule, unchanged.
    #[cfg(windows)]
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

    /// The three ways WSL can be absent reach the list as three different sentences, which
    /// is more than the catalogue's single generic one could say. A user who has never
    /// installed WSL and one whose WSL is broken must not be read the same thing.
    #[cfg(windows)]
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

    /// The debug gate, from this side: what is listed is what the composition root supplied,
    /// and a build that supplies nothing lists nothing. A release build passes an empty
    /// list, so this is the same assertion the gate makes.
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

    /// The list is asked of the machine every time, because a distribution installed while
    /// Acter is open must appear without a restart (decision 6).
    #[test]
    fn the_list_asks_the_machine_again_on_every_call() {
        struct Counting(AtomicUsize);
        impl InstalledShells for Counting {
            fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(NoDistributions::NotInstalled)
            }
            fn is_available(&self, _program: &str) -> bool {
                true
            }
        }
        let machine = Arc::new(Counting(AtomicUsize::new(0)));
        let service = ConnectService::new(
            Arc::new(FakeFactory::default()),
            Arc::clone(&machine) as Arc<dyn InstalledShells>,
            Vec::new(),
        );

        service.connectable();
        let after_one = machine.0.load(Ordering::SeqCst);
        service.connectable();

        assert!(
            machine.0.load(Ordering::SeqCst) > after_one,
            "a cached list is a list that is wrong without saying so"
        );
    }

    /// A window that has not been connected to anything says so to every question it is
    /// asked, and the two a user can actually ask are these.
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

    /// Using one makes it the session: it is what a submitted line reaches, and it is what
    /// the window is told it is on.
    #[test]
    fn using_a_profile_makes_it_the_session_a_line_reaches() {
        let (service, factory) = service(FakeMachine::complete(), &[]);
        let id = ProfileId::Shell {
            kind: ConnectionKind::Cmd,
        };

        let connected = service.use_profile(&id, &unasked()).expect("cmd starts");

        assert_eq!(connected.label, "Command Prompt");
        assert_eq!(service.connected(), Some(connected.clone()));
        assert_eq!(
            service.submit_command(connected.session, "dir"),
            SubmitAck::Accepted {
                command_id: CommandId(1)
            }
        );
        assert_eq!(*factory.opened.lock().unwrap(), [id]);
    }

    /// **Decision 4's teeth.** Connecting twice leaves exactly one session alive, because
    /// the outgoing one is really let go rather than merely forgotten. Against a real
    /// transport this is what kills the shell; here it is the last handle going, which is
    /// the thing that has to happen for that to follow.
    #[test]
    fn replacing_a_session_lets_the_previous_one_go() {
        let (service, factory) = service(FakeMachine::complete(), &[]);

        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                &unasked(),
            )
            .expect("cmd starts");
        assert_eq!(factory.alive.load(Ordering::SeqCst), 1);

        service
            .use_profile(
                &ProfileId::Distribution {
                    name: "Ubuntu".to_owned(),
                },
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

    /// The id is minted per connection and it is load-bearing: a line submitted to the
    /// session that was just replaced is refused rather than run in the new one, in a
    /// working directory and on a machine the user never chose for it.
    #[test]
    fn a_line_for_the_replaced_session_is_refused_rather_than_run_in_the_new_one() {
        let (service, _) = service(FakeMachine::complete(), &[]);

        let first = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                &unasked(),
            )
            .expect("cmd starts");
        let second = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
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

    /// **Decision 5.** A failure costs the user nothing — not their place, and not even the
    /// shell they were in — and what they hear is the factory's own sentence.
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
            Vec::new(),
        );
        let working = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                &unasked(),
            )
            .expect("cmd starts");

        let why = service
            .use_profile(&refused, &unasked())
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

    /// A kind the machine does not have is refused before anything is spawned, with the
    /// instructions the list would have shown — the same words in both places, so a user
    /// who chose the row and a user who read the panel are told the same thing.
    #[test]
    fn using_something_this_machine_lacks_says_what_to_install() {
        let (service, factory) = service(FakeMachine::without("pwsh.exe"), &[]);

        let why = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::PowerShellSeven,
                },
                &unasked(),
            )
            .expect_err("PowerShell 7 is not installed");

        assert_eq!(why, ConnectionKind::PowerShellSeven.instructions());
        assert!(
            factory.opened.lock().unwrap().is_empty(),
            "nothing is spawned to find out what the machine already answered"
        );
    }

    /// And the same for WSL, which answers with its own reason rather than the catalogue's
    /// generic one.
    #[test]
    fn using_a_distribution_on_a_machine_without_wsl_says_wsls_own_reason() {
        let (service, _) = service(FakeMachine::without_wsl(NoDistributions::NotInstalled), &[]);

        let why = service
            .use_profile(
                &ProfileId::Distribution {
                    name: "Ubuntu".to_owned(),
                },
                &unasked(),
            )
            .expect_err("there is no WSL to start it in");

        assert_eq!(why, NoDistributions::NotInstalled.to_string());
    }

    /// Attaching reaches the live session, and reaching nothing is not a panic: a frontend
    /// that attaches to a window with no session is an ordinary launch, not an error.
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

    /// A sink attached with the id of a session that is gone reaches nothing rather than
    /// the session that replaced it — the same rule a submitted line follows, for the same
    /// reason.
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
                &unasked(),
            )
            .expect("cmd starts");
        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
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
}
