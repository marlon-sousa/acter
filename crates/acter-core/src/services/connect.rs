//! Service: `ConnectService` — which session this window is on, and how it becomes a
//! different one.
//!
//! It coordinates three things for one named use case: the catalogue policy, which decides
//! what belongs in a connect list and in what order; [`ThisComputer`], which answers
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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    Chosen, ConnectApi, ConnectQuestions, Connectable, Connected, Connection, ConnectionKind,
    EventSink, KeyAck, KeyPress, ProfileId, ProgramAnswer, ProgramQuestion, SessionApi,
    SessionFactory, SessionId, SetUp, ShellInstall, Signatures, Started, SubmitAck, ThisComputer,
    Variant, catalogue,
};

/// The port every SSH server listens on unless somebody moved it, which is what the form
/// starts filled in with.
const DEFAULT_SSH_PORT: u16 = 22;

/// The one session, and everything needed to replace it.
pub struct ConnectService {
    factory: Arc<dyn SessionFactory>,
    machine: Arc<dyn ThisComputer>,
    /// Who signed the files this machine would start.
    ///
    /// **Asked on the connection and not when the list is drawn** (spec B5.7, decision 7).
    /// `WinVerifyTrust` is not free and revocation can reach the network, and the list is
    /// built on open in front of a listener who is waiting. What the list does ask for is
    /// [`Signatures::known`], which answers from what has already been paid for and never
    /// verifies anything.
    signatures: Arc<dyn Signatures>,
    /// The scripted far ends this build offers, in the order they are listed.
    ///
    /// **Supplied rather than known** (spec B7, decision 7). What a scripted session *is* —
    /// a transcript, a chunking, a decorator over either — is a transport composition, and
    /// the domain has no business holding four names for compositions it cannot build. The
    /// composition root passes them, and passes none at all in a release build, so the gate
    /// is where the construction is.
    scripted: Vec<String>,
    /// The connection kinds this operating system offers, in the order a listener meets
    /// them.
    ///
    /// **Supplied rather than known**, for [`Self::scripted`]'s reason one platform later
    /// (M1). The list itself is `offered`'s, a policy over the operating system's name; what
    /// the composition root does is read which operating system this is, because reading the
    /// environment is the composition root's privilege and not a service's.
    ///
    /// It also buys the tests below their platform independence: a fake machine can be asked
    /// for the Windows list on a Mac and the macOS list on Windows, where the `#[cfg]`
    /// -selected constant this replaces made half of connecting unassertable on either.
    kinds: Vec<ConnectionKind>,
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
    /// Whether that note already said this session cannot report how a command went, kept for
    /// the same reason the note is: a window asking again gets the same answer.
    limit_explained: bool,
    session: Arc<dyn SessionApi>,
}

impl ConnectService {
    /// A window connected to nothing, which is what an ordinary launch now opens.
    pub fn new(
        factory: Arc<dyn SessionFactory>,
        machine: Arc<dyn ThisComputer>,
        signatures: Arc<dyn Signatures>,
        kinds: Vec<ConnectionKind>,
        scripted: Vec<String>,
    ) -> Self {
        Self {
            factory,
            machine,
            signatures,
            kinds,
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

    /// The install this machine would start for a program named by a kind, or `None` when it
    /// has none.
    ///
    /// **The first one, because the adapter answers most preferred first** — and what makes
    /// one preferred is `PATH`, which knows the one thing no other source does: what the
    /// name means to this user (spec B5.7, decision 2).
    fn install(&self, program: &str) -> Option<ShellInstall> {
        self.machine.installs(program).into_iter().next()
    }

    /// What this machine would start for this profile, resolved once — or the sentence to
    /// say instead.
    ///
    /// **This is decision 1.** Until B5.7 the machine was asked whether a *name* could be
    /// started and the transport later started that same name, so Windows resolved it a
    /// second time and nothing guaranteed the two resolutions landed on the same file. Now
    /// the file is found here, it is what gets verified, and it is what
    /// [`SessionFactory::open`] is handed.
    ///
    /// Asked *before* the factory, so a kind the catalogue already reported as missing is
    /// refused with the instructions the catalogue would have shown rather than with
    /// whatever a failed spawn happened to say.
    fn chosen(&self, id: &ProfileId) -> Result<Chosen, String> {
        let program = match id {
            // WSL is available when it can name a distribution, and its three ways of not
            // being available are three different sentences (spec B5.3, decision 6) — all
            // of them better than the catalogue's generic one, which has to serve a machine
            // nobody has asked yet. What is *started* either way is the client, so that is
            // the file resolved and verified.
            ProfileId::Shell {
                kind: ConnectionKind::Wsl,
            }
            | ProfileId::Distribution { .. } => {
                self.machine
                    .wsl_distributions()
                    .map_err(|why| why.to_string())?;
                Some(self.found(ConnectionKind::Wsl)?)
            }
            // A kind that comes in editions is startable when any of them is, and what
            // starts is the first that can — the same answer the row's own id carries.
            ProfileId::Shell {
                kind: kind @ ConnectionKind::PowerShell,
            } => Some(
                kind.editions()
                    .iter()
                    .find_map(|edition| self.install(edition.program()))
                    .map(|install| install.program)
                    .ok_or_else(|| kind.instructions().to_owned())?,
            ),
            // **The account's own shell, resolved here for the reason WSL's client is**:
            // the row's id already carries it, and this arm is what answers a profile that
            // named the kind alone — `--profile` on the command line, or a saved one from
            // B8. A machine that lists none is refused with the kind's own sentence.
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
            // **Already resolved, by the list that offered it.** Resolving it again here
            // would be the second resolution this entry exists to remove.
            ProfileId::Install { program, .. } => Some(PathBuf::from(program)),
            // **Nothing on this machine to check** — Acter speaks SSH itself — so the only
            // thing that can be wrong here is the form. An empty host is refused with a
            // sentence rather than handed to the transport, which would answer with a
            // network error about a name nobody typed.
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
            // A program named directly is not in the catalogue and has no instructions to
            // offer, so the answer to a name that does not start is the factory's — which
            // is the transport's own sentence about the spawn that failed.
            //
            // **Resolved when it resolves, and left alone when it does not.** A name this
            // machine really has is a file, so it is checked and started like any other; a
            // name nothing resolves has no file to check, and answering "Acter could not
            // check who signed this" about something that was never there would replace the
            // transport's plain sentence with a security one about nothing.
            ProfileId::Program { program } => {
                self.install(program.trim()).map(|install| install.program)
            }
            // A composition rather than a file, so there is nothing to resolve and nothing
            // to verify.
            ProfileId::Scripted { .. } => None,
        };
        Ok(Chosen {
            profile: id.clone(),
            program,
        })
    }

    /// The file for a kind that is one program, or that kind's own instructions.
    fn found(&self, kind: ConnectionKind) -> Result<PathBuf, String> {
        self.install(kind.program())
            .map(|install| install.program)
            .ok_or_else(|| kind.instructions().to_owned())
    }

    /// Verify the file that is about to be started, and ask about it when Windows would not
    /// vouch for it.
    ///
    /// Returns the clause to say once at connection: `None` for a file Microsoft signed,
    /// because **a verdict nobody needs to act on is not an announcement**; something to say
    /// for every other outcome the user agreed to, so nobody is left unsure what they just
    /// agreed to (spec B5.7, accessibility checklist).
    ///
    /// **Never a gate** (decision 6). The only thing that can stop a connection here is the
    /// person in front of the window saying so.
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
            // The verdict's own sentence, which already ends in what to do next — so a
            // listener who declined and wants to reconsider is told the same thing twice
            // rather than told a second, differently worded thing.
            ProgramAnswer::DoNotStart => {
                Err(format!("Acter did not start {label}. {}", verdict.said()))
            }
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
    ///
    /// **Its id names the kind rather than a file**, unlike every other shell row since
    /// B5.7: what a WSL row starts is a distribution, and `wsl.exe` is the client that gets
    /// it there. That client is resolved and verified on the connection like any other
    /// program this machine runs.
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
    /// variants (spec A11) — and since B5.7, one variant per *install* of an edition.
    ///
    /// **A missing edition stays in the panel and says what to do about it**, which is the
    /// difference from WSL and the reason [`Variant`] carries availability at all. A machine
    /// with Windows PowerShell and no PowerShell 7 has the kind, and listing only what is
    /// installed would teach that listener that Acter does not support the edition they
    /// have read about — which is precisely B5.4's argument, one level down.
    ///
    /// **A machine with one PowerShell 7 sees exactly what it saw before** (decision 9):
    /// nothing is added to an entry until there is another one to tell it from.
    ///
    /// The row is available when *any* edition is, and the id it carries is the first
    /// install that can actually be started, so choosing the row without opening the panel
    /// starts something rather than failing.
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

    /// The connect list's Terminal row: one row for the kind, carrying the shells
    /// `/etc/shells` names as its variants, the account's own first and marked as the
    /// default (spec M2, decision 2).
    ///
    /// **The shape A11 gave PowerShell and A8 gave WSL, on the platform they were designed
    /// for without being designed for it.** A listener arrowing the kinds meets "Terminal"
    /// once, however many shells this Mac happens to have — which on a stock macOS install
    /// is seven, and seven rows for one idea is exactly what the variants panel exists to
    /// prevent.
    ///
    /// **Its id is the default variant's, so Enter with nothing chosen starts what a
    /// Terminal.app window would have started.** That is the whole reason the account's
    /// login shell is read at all: a row that started the first line of `/etc/shells` would
    /// start `/bin/bash` for a zsh user, and say nothing about having done so.
    ///
    /// **Every variant is available, for the reason a WSL distribution is**: a shell that is
    /// not there cannot be listed by the file that lists what is there. What a variant can
    /// carry is a verdict somebody already paid for, exactly as a PowerShell install does.
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
                        // **The shell's own name, which is what tells one variant from
                        // another here** — `Provenance`'s job on Windows, done by the file
                        // name on a machine where every one of these lives in the same
                        // directory and differs only by what it is.
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

    /// What an entry is called, with a verdict already paid for carried in its **name**.
    ///
    /// **This is how decisions 6 and 7 fit together.** Nothing is verified to draw the list,
    /// so an entry nobody has tried to start says exactly what it said before. An entry that
    /// failed to verify the last time somebody tried carries that, the way A11's missing
    /// edition carries its own absence — because a greyed-out entry that looks different and
    /// reads the same is the failure this product exists to avoid.
    /// **The label is passed in rather than taken from the id, since M2.** A PowerShell
    /// variant is called what its profile is called; a Terminal variant is called `zsh`,
    /// which is the shell's own name and not the profile's — so the caller decides what is
    /// being qualified and this decides only whether a verdict qualifies it.
    fn named(&self, label: String, program: &Path) -> String {
        match self.signatures.known(program) {
            Some(verdict) if !verdict.settled() => format!("{label}{NOT_VERIFIED}"),
            _ => label,
        }
    }
}

/// What tells the installs of one edition apart, in the order they were found.
///
/// **Nothing at all when there is only one**, which is decision 9's other half: the machine
/// with a single PowerShell 7 hears "PowerShell 7", as it does today. Where there is more
/// than one, each is named by where it came from — and where two provenances would say the
/// same thing, by the directory each lives in, which is always different because two files
/// cannot be the same file in two places.
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

/// The suffix an unavailable variant carries, in its **name** rather than in a visual state,
/// for the reason [`catalogue`](crate::catalogue) gives for a row: a greyed-out entry that
/// looks different and reads the same is the failure this product exists to avoid.
const NOT_AVAILABLE: &str = " (not available)";

/// What marks the shell this account itself logs in to, in its **name** for
/// [`NOT_AVAILABLE`]'s reason: a variant that is chosen for you when you press Enter has to
/// say so out loud, because nothing about the order of a list is audible.
const DEFAULT: &str = " (default)";

/// The same, for an entry that is installed and did not verify when somebody last tried to
/// start it (spec B5.7, decision 6). **It is not a filter**: the entry keeps its place in
/// the list, and choosing it asks a question rather than refusing.
const NOT_VERIFIED: &str = " (not verified)";

impl ConnectApi for ConnectService {
    /// The catalogue, asked of this machine, with WSL carrying its distributions and the
    /// scripted sessions appended.
    ///
    /// **The machine is asked here rather than remembered**, twice over: `wsl.exe` is run
    /// and every program is looked up on every call. That is the cost of a list that is
    /// true when a user opens it (decision 6).
    ///
    /// **Nothing is verified here and nothing reaches the network** (spec B5.7, decision 7).
    /// The list is built on open, in front of a listener who is waiting, and a revocation
    /// check that hits a network timeout there would be 23.7's objection one entry later.
    ///
    /// The scripted sessions go last, after everything real, because they are a developer's
    /// tools and a user arrowing this list should meet their own shells first.
    fn connectable(&self) -> Vec<Connectable> {
        let mut listed: Vec<Connectable> = catalogue(&self.kinds, |kind| match kind {
            ConnectionKind::Wsl => self.machine.wsl_distributions().is_ok(),
            // A kind that comes in editions is available when any of them is: a machine with
            // PowerShell 7 and no Windows PowerShell still has PowerShell.
            ConnectionKind::PowerShell => ConnectionKind::PowerShell
                .editions()
                .iter()
                .any(|edition| !self.machine.installs(edition.program()).is_empty()),
            // **Never asked of the machine, because it is not on the machine.** Acter
            // speaks SSH itself (spec B9, decision 1), so there is no executable to look
            // for — and looking for one found nothing and offered every user
            // "SSH (not available)", which is the opposite of true.
            ConnectionKind::Ssh => true,
            // **Asked of the file that lists them rather than of `PATH`**, because what a
            // Terminal row offers is the shells an account may log *in* to, which is
            // `/etc/shells`' answer and nobody else's (spec M2, decision 3).
            ConnectionKind::Terminal => !self.machine.login_shells().is_empty(),
            other => !self.machine.installs(other.program()).is_empty(),
        })
        .iter()
        .map(|row| match row.kind {
            ConnectionKind::Wsl => self.wsl_row(row),
            ConnectionKind::PowerShell => self.powershell_row(row),
            ConnectionKind::Terminal => self.terminal_row(row),
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

    /// **The order of operations is the decision** (spec B7, decision 5, and B5.7 decision
    /// 1): the machine is asked, the file it named is verified, the new session is built,
    /// and only then is the old one let go. A failure at any of the first three steps
    /// returns before anything has been replaced, so the session the user was in is still
    /// running and still attached.
    ///
    /// Letting go is what ends the outgoing shell. Dropping the last `Arc` drops the
    /// request channel the pump is selecting on, the pump breaks out of its loop, and the
    /// transport it owns is dropped with it — which for a local one kills the process. It
    /// happens *outside* the lock, so a shell taking its time to die does not hold up a
    /// window that is already on the next session.
    fn use_profile(
        &self,
        id: &ProfileId,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Connected, String> {
        let chosen = self.chosen(id)?;
        let label = id.label();
        let agreed = self.verified(&chosen, &label, questions)?;
        // **Every question, rather than the SSH half, since B9.5.** The setup question is
        // asked after the connection succeeds and before the setup line is sent, and that
        // window is inside the factory's call rather than in front of it: the far end has to
        // have said what shell it runs before a dialog can name it.
        let Started {
            session,
            note,
            limit_explained,
        } = self.factory.open(&chosen, set_up, questions)?;

        // **At most one of the two is ever present**: a note from the far end belongs to
        // SSH, which is not a file on this machine, and a note about a signature belongs to
        // a file, which SSH does not have. The far end's comes first all the same, because
        // it is about what the user is now talking to rather than about how it was started.
        let note = note.or(agreed);
        let next = SessionId(self.next.fetch_add(1, Ordering::SeqCst));
        let previous = {
            let mut current = self.current.lock().expect("session lock poisoned");
            current.replace(Live {
                id: next,
                label: label.clone(),
                note: note.clone(),
                limit_explained,
                session,
            })
        };
        drop(previous);

        Ok(Connected {
            session: next,
            label,
            note,
            limit_explained,
        })
    }

    fn connected(&self) -> Option<Connected> {
        let current = self.current.lock().expect("session lock poisoned");
        current.as_ref().map(|live| Connected {
            session: live.id,
            label: live.label.clone(),
            note: live.note.clone(),
            limit_explained: live.limit_explained,
        })
    }
}

impl ConnectService {
    /// A kind that is one program: the row names the file the list resolved, so what is
    /// verified on the connection is what this row already found (decision 1).
    fn shell_row(&self, row: &Connection, kind: ConnectionKind) -> Connectable {
        match self.install(kind.program()) {
            Some(install) => {
                let id = ProfileId::Install {
                    kind,
                    program: install.program.display().to_string(),
                    // One kind, one program, so there is never another to tell it from.
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

    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;

    use crate::{
        CommandId, Fault, HostKeyAnswer, HostKeyQuestion, LoginShell, NoDistributions,
        PasswordQuestion, PathStanding, Provenance, Secret, SessionEvent, SetupAnswer,
        SetupQuestion, Signer, SshQuestions, Verdict,
    };

    use super::*;
    // **Windows' kinds, named rather than inherited from the build** (M1). Every fake in
    // this module is a Windows machine — cmd in `system32`, two PowerShell editions, WSL
    // distributions — so the list asked for is the one those fakes are answers to, and it
    // is asked for on whichever platform this suite happens to be running on. Eleven tests
    // below used to be `#[cfg(windows)]` for want of exactly this line.
    use crate::offered;

    /// Nobody to ask, for every test here whose subject is not the asking.
    fn unasked() -> Arc<dyn ConnectQuestions> {
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
        /// Everything it was asked to start — the profile **and the file**, which is how
        /// "the path verified is the path started" is asserted through the port.
        opened: Mutex<Vec<Chosen>>,
        /// The last session handed out, so a test can ask it what reached it.
        last: Mutex<Option<Arc<FakeSession>>>,
        /// A profile the factory refuses, with the sentence it refuses it in.
        refuses: Mutex<Option<(ProfileId, String)>>,
        /// What the Connect dialog's checkbox said on each attempt, so a test can assert that
        /// the answer reached the thing that acts on it (spec B9.5, decision 9).
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

    /// A machine with exactly what a test says it has.
    struct FakeMachine {
        programs: Vec<&'static str>,
        distributions: Result<Vec<String>, NoDistributions>,
        /// The shells an account here may log in to, the default named separately — empty
        /// for the Windows machine every other test in this module is about.
        shells: Vec<&'static str>,
        mine: Option<&'static str>,
        /// Installs spelled out by a test, for the machine that has more than one of
        /// something — which is the case the ordinary one cannot express.
        extra: HashMap<&'static str, Vec<ShellInstall>>,
    }

    impl FakeMachine {
        /// Everything installed, two distributions.
        fn complete() -> Self {
            Self {
                programs: vec!["cmd.exe", "powershell.exe", "pwsh.exe", "wsl.exe"],
                distributions: Ok(vec!["Ubuntu".to_owned(), "Debian".to_owned()]),
                shells: Vec::new(),
                mine: None,
                extra: HashMap::new(),
            }
        }

        /// A Mac: no Windows programs at all, seven shells, and an account that logs in to
        /// one of them.
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

        /// The same Mac with nothing an account can log in to, which is the one way a
        /// Terminal row can be unavailable.
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

        /// A machine where one program resolves to more than one file.
        fn with(mut self, program: &'static str, installs: Vec<ShellInstall>) -> Self {
            self.extra.insert(program, installs);
            self
        }
    }

    impl ThisComputer for FakeMachine {
        fn wsl_distributions(&self) -> Result<Vec<String>, NoDistributions> {
            self.distributions.clone()
        }

        /// The account's own shell first, as the adapter answers, so the service is tested
        /// against the order it will really be handed.
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

        /// One install per program this machine has, in the place Windows keeps it — plus
        /// whatever a test added by hand for the machine with two of something.
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

        /// **Nothing, and the assertion is that nothing calls it.** Which shell a
        /// distribution runs is asked once per connection and never while the list is built
        /// (spec B5.5, decision 3) — asking per row would start one `wsl.exe` for every
        /// distribution every time the connect dialog opens, in the very place a listener is
        /// already waiting. This service does not connect, so it does not ask.
        fn login_shell(&self, _distribution: Option<&str>) -> Option<String> {
            None
        }
    }

    /// Signatures a test decides, keyed by the file — and a record of every file anything
    /// asked about, which is how "the list verifies nothing" is asserted.
    #[derive(Default)]
    struct FakeSignatures {
        verdicts: Mutex<Vec<(PathBuf, Verdict)>>,
        verified: Mutex<Vec<PathBuf>>,
        /// What is already known without verifying, for the list that names a verdict it
        /// did not pay for.
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

    /// Somebody who says yes to everything they are asked about a file, and refuses every
    /// question that is not about one — so a test of starting anyway cannot pass by
    /// accident.
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

    /// The one question this entry asks, remembered so a test can read it back.
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

    /// The same, with the signatures a test decides — and the fake handed back, so a test
    /// can ask it what it was made to check.
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
            // **Windows' kinds, named rather than inherited from the build** (M1). Every
            // fake below is a Windows machine — cmd in `system32`, two PowerShell editions,
            // WSL distributions — so this asks for the list those fakes are answers to,
            // and it asks for it on whichever platform the suite happens to be running on.
            offered("windows").to_vec(),
            scripted.iter().map(|name| (*name).to_owned()).collect(),
        );
        (Arc::new(service), factory, signatures)
    }

    /// The same, for a Mac — **the platform named rather than inherited from the build**,
    /// for `signed`'s reason (M1): the fake below is a Mac, so this asks for the list a Mac
    /// is offered, and it asks for it on whichever machine the suite happens to run on.
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
        );
        (Arc::new(service), factory, signatures)
    }

    /// One row of a list, by kind.
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

    /// One install, spelled out by a test.
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

    /// The acceptance criterion, spelled out: cmd, both editions, WSL once, and the
    /// scripted sessions last.
    ///
    /// **SSH is offered on every machine**, because there is nothing to install: Acter
    /// speaks the protocol itself. The row connects to nothing until the dialog's form has
    /// been filled in, which is what makes it the one kind that is a form rather than a
    /// choice (spec A8, decision 1).
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
            let Err(why) = service.use_profile(&profile, SetUp::Yes, &unasked()) else {
                panic!("an unfilled form does not connect");
            };
            assert!(why.contains(expected), "it says what is missing: {why}");
            assert!(why.ends_with('.'), "a spoken message ends: {why}");
        }
    }

    /// **One row for WSL rather than one per distribution** since A8: the dialog has a
    /// panel to put them in, so a listener arrows the kinds and meets "WSL" once however
    /// many distributions this machine has.
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

    /// A machine with neither edition has the kind, unavailable, saying so in its name — the
    /// row-level rule, unchanged.
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

    /// **The acceptance criterion of M2, spelled out.** A Mac meets two rows, the first is
    /// Terminal, and its panel holds the shells this machine has with the account's own
    /// first and marked.
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

    /// **Enter on the row with nothing chosen starts what a Terminal.app window would have
    /// started** (spec M2, decision 2), which is the whole reason the account's login shell
    /// is read at all rather than the first line of the file being taken.
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

    /// **A shell is a variant and never a row** (spec A11, and DESIGN's macOS section). A
    /// listener arrowing the kinds meets Terminal once, however many shells this Mac has —
    /// which on a stock install is seven.
    #[test]
    fn a_shell_on_this_mac_is_never_a_row_of_its_own() {
        let (service, _, _) = on_a_mac(FakeMachine::a_mac(), Arc::new(FakeSignatures::default()));

        let listed = service.connectable();

        assert_eq!(listed.len(), 2, "seven shells did not become seven rows");
        assert_eq!(row(&listed, ConnectionKind::Terminal).variants.len(), 7);
    }

    /// **What a Mac is never offered.** cmd, PowerShell and WSL are absent rather than
    /// unavailable: a Mac read instructions to install Windows is the absurdity the
    /// not-available label exists to avoid where it means something.
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

    /// The one way a Terminal row can be unavailable, and it says what to look at rather
    /// than going quiet — the same shape a missing WSL has.
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

    /// **A verdict already paid for is carried in the variant's name**, exactly as a
    /// PowerShell install carries one (spec B5.7, decision 6) — and the shell's own name
    /// survives in front of it, because that is what a listener is choosing between.
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

    /// **Nothing is verified to draw the list** (decision 7), on this platform as on the
    /// other: the panel is built in front of a listener who is waiting.
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

    /// **The file the list resolved is the file that is started** (spec B5.7, decision 1),
    /// which on a Mac is what makes the verdict about anything: a row that named `zsh` and
    /// let something else resolve it later would be checking one file and starting another.
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
            .use_profile(&bash.id, SetUp::Yes, &unasked())
            .expect("a shell this Mac has starts");

        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from("/bin/bash")),
            "the file, not the name"
        );
    }

    /// And the kind on its own — what `--profile` and B8's saved profiles will carry —
    /// resolves to the account's own shell rather than to nothing.
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
                &unasked(),
            )
            .expect("the account's own shell starts");

        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from("/bin/zsh"))
        );
    }

    /// **A Mac with nothing to log in to refuses with the kind's own sentence**, rather than
    /// with whatever a failed spawn happened to say — which is `chosen`'s rule, applied to
    /// the one kind that can be empty here.
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
                &unasked(),
            )
            .expect_err("there is nothing to start");

        assert!(refused.contains("/etc/shells"), "{refused}");
        assert!(
            factory.opened.lock().unwrap().is_empty(),
            "and nothing was started"
        );
    }

    /// The list is asked of the machine every time, because a distribution installed while
    /// Acter is open must appear without a restart (decision 6).
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

        let connected = service
            .use_profile(&id, SetUp::Yes, &unasked())
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
                SetUp::Yes,
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
                SetUp::Yes,
                &unasked(),
            )
            .expect("cmd starts");
        let second = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
                SetUp::Yes,
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
            Arc::new(FakeSignatures::default()),
            offered("windows").to_vec(),
            Vec::new(),
        );
        let working = service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::Cmd,
                },
                SetUp::Yes,
                &unasked(),
            )
            .expect("cmd starts");

        let why = service
            .use_profile(&refused, SetUp::Yes, &unasked())
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
                SetUp::Yes,
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
                SetUp::Yes,
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
                SetUp::Yes,
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
                SetUp::Yes,
                &unasked(),
            )
            .expect("cmd starts");
        service
            .use_profile(
                &ProfileId::Shell {
                    kind: ConnectionKind::WindowsPowerShell,
                },
                SetUp::Yes,
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

    /// **The whole of decision 1, asserted through the port.** The machine names a file, the
    /// signature port is asked about that file, and the factory is handed that file — so
    /// nothing between the check and the spawn resolves a name a second time.
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

    /// **Decision 7, and the reason the verdict is not on the row.** `WinVerifyTrust` is not
    /// free and revocation can reach the network; this list is built on open, in front of a
    /// listener who is waiting, and 23.7 already refused to put work there.
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

    /// **Decisions 6 and 7 fitting together.** Nothing is verified to draw the list, and an
    /// entry that failed the last time somebody tried to start it says so in its **name** —
    /// the way A11's missing edition does, because a greyed-out entry that looks different
    /// and reads the same is the failure this product exists to avoid.
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

    /// **Never a gate** (decision 6), the whole shape in one test: an untrusted file is
    /// offered, choosing it asks a question naming the file and who signed it, and the
    /// default answer starts nothing and leaves the session the user was in alone.
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

    /// **And agreeing starts it and says what was agreed to** (accessibility checklist):
    /// nobody should be unsure what they just consented to.
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

    /// **A verdict nobody needs to act on is not an announcement** (accessibility checklist).
    /// Connecting to a shell Windows ships says exactly what it said before this entry.
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
                &unasked(),
            )
            .expect("cmd starts");

        assert_eq!(connected.note, None);
    }

    /// **Trusted and signed by somebody else is a different sentence** (decision 5) — not a
    /// question, because this machine's own trust store accepts it, and one clause so nobody
    /// is left thinking Microsoft built their shell.
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

    /// **Decision 9.** Two PowerShell 7 installs are two entries a listener can tell apart
    /// without opening either, and each carries the file that is it.
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

    /// **The other half of decision 9, and the one most machines are.** One install of an
    /// edition is named exactly as it was before this entry existed: A11's panel, unchanged.
    #[test]
    fn a_machine_with_one_powershell_seven_reads_as_it_did_before() {
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

    /// **An install that says nothing about itself is not guessed at** (decision 3). Where it
    /// came from is unknowable, so what tells it from the other one is the only fact there
    /// is: the directory it lives in.
    ///
    /// **The directories are spelled by the platform running this, not written out** (M1).
    /// `C:\tools\pwsh\pwsh.exe` is one filename off Windows rather than a path — `Path::parent`
    /// finds no separator in it — so a literal expectation here would assert Windows' spelling
    /// rather than the rule, which holds wherever an install says nothing about itself.
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

    /// Choosing an install starts *that* install, rather than whatever the name would have
    /// resolved to a second time — the same rule as decision 1, seen from the panel.
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
            .use_profile(&chosen, SetUp::Yes, &unasked())
            .expect("the chosen install starts");

        assert_eq!(connected.label, r"PowerShell 7 (C:\tools\pwsh)");
        assert_eq!(
            factory.opened.lock().unwrap()[0].program,
            Some(PathBuf::from(r"C:\tools\pwsh\pwsh.exe"))
        );
    }
}
