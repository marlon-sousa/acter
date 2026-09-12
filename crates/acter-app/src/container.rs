//! Controller (orchestrator): the container (composition root) — the only place
//! where concrete implementations are constructed and bound to their ports, where the
//! environment is read, and where the Tauri runtime is started.
//!
//! Since B6 the wired backend is the real thing: `SessionService` over a byte transport,
//! a real terminal engine, the real boundary tracker, the real pacing policy and the real
//! session actor. What is scripted is the far end of the *transport* and nothing above it
//! — faking is a transport choice now, which is why there is exactly one session service
//! and why `FakeSessionService` was deleted rather than kept beside it (spec B6,
//! decision 12).
//!
//! **Since B7 it builds no session at all.** It builds the *factory* — the one thing that
//! knows a `LocalPty`, an `AlacrittyEngine` and a `SessionService` belong to each other —
//! and hands it to `ConnectService`, which decides when a session exists. An ordinary
//! launch now opens a window connected to nothing; a launch that names a shell connects to
//! it through the same action a menu would.

use std::env;
use std::env::consts;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use acter_core::{
    Chosen, Clock, ConnectApi, ConnectQuestions, ConnectService, ConnectionKind, ConnectionStore,
    Explained, HostKeyStore, PacingConfig, ProfileId, SessionApi, SessionFactory, SessionService,
    SetUp, SetupAnswer, SetupQuestion, ShellAdapter, ShellFacts, ShellLaunch, Signatures,
    SshQuestions, Started, ThisComputer, Transport, Unasked, offered,
};
#[cfg(target_os = "macos")]
use acter_shells::AppleTrust;
#[cfg(not(windows))]
use acter_shells::UnixMachine;
#[cfg(windows)]
use acter_shells::WindowsMachine;
#[cfg(windows)]
use acter_shells::WindowsTrust;
use acter_shells::{Plain, UnixShell, Wsl, adapter_for, is_wsl};
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, KnownHosts, LocalPty, ScriptedTransport, SessionTranscript, SshTarget,
    SshTransport, TranscriptShell, Unmarked, probe_patience,
};
use tauri::{Builder, generate_context, generate_handler};

use crate::adapters::{ExplainedShells, Settings, SystemClock, install_system_menu};
use crate::controllers::Connecting;

/// The environment variable choosing which simulated session to run: a built-in name, or
/// a path to a transcript JSON.
///
/// **Retired by B8**, along with [`SHELL_ENV`]: `acter --profile <name>` is what replaces
/// both, and until it exists the suites and the manual accessibility passes need a way to
/// say which session they are testing (spec B7, decision 7).
const TRANSCRIPT_ENV: &str = "ACTER_TRANSCRIPT";

/// The environment variable naming a real shell to run instead of a scripted session:
/// a program for `LocalPty` to spawn, `cmd.exe` or `powershell.exe` for instance.
///
/// **Three shells are integrated now: `cmd.exe`, either PowerShell, and `wsl.exe`.**
/// Naming cmd gets the OSC 133 prompt injection and boundaries around the prompt and the
/// command line, and nothing about output or exit codes (spec B4.5). Naming `powershell` or
/// `pwsh` gets the snippet injection and the full cycle including a real exit code (spec
/// B5.2); naming wsl gets a bash session in whatever distribution WSL calls the default,
/// marking the full cycle too (spec B5.3). Naming any other shell still gets a session with
/// no integration at all, degrading exactly as DESIGN's reliability case 2 says it should.
/// Which of those a name resolves to is `acter_shells::adapter_for`'s since B5.1, not this
/// file's.
///
/// **Neither variable set opens a window connected to nothing**, which since B7 is what an
/// ordinary launch is. It used to be the scripted transcript, and it stopped being that the
/// moment connecting became something a user does rather than something a launch decided.
const SHELL_ENV: &str = "ACTER_SHELL";

/// The emulated screen the engine keeps. Eighty by twenty-four, the same as the
/// automated suite uses, so a manual session scrolls where the tests say it scrolls.
/// A real transport is opened at this size too, so the emulator and the far end agree
/// from the first byte; a window-driven resize is still nobody's yet.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

/// What a scripted session is started as, for the one place a name is still needed: the
/// null adapter carries the program it was selected by, and no process is ever spawned
/// from this one.
const SCRIPTED: &str = "scripted";

/// The WSL client, for the one profile that names a distribution rather than a program.
///
/// Spelled here rather than taken from `ConnectionKind::Wsl.program()` because they answer
/// different questions — that one is what the *machine* is asked about — but they are the
/// same string, and `acter_shells::wsl::is_wsl` recognises either spelling.
const WSL_CLIENT: &str = "wsl.exe";

/// Where Acter keeps everything it writes, when somebody says (spec 26, decision 3).
///
/// **It wins over the packaging, exactly as `ACTER_PROFILES_DIR` did**: it is what points
/// the suites and the NVDA fixture at a directory made for them, and a manual pass whose
/// saved connections depend on this machine's history is not repeatable. It is also the one
/// way to keep settings somewhere the packaging did not choose, which is what converting a
/// copy by hand means now. An empty value is ignored rather than treated as a path, so a
/// variable somebody cleared rather than unset does not send Acter's records to the root of
/// the filesystem.
///
/// **Nothing shipped reads the old name, so there is no alias** (decision 1).
const SETTINGS_DIR: &str = "ACTER_SETTINGS_DIR";

/// The name of the settings folder, wherever it turns out to be (decision 2).
///
/// **It is a name and not a signal.** An earlier version of decision 3 made the *presence*
/// of a folder with this name beside the program mean the copy was portable, and that
/// inferred a fact about the package from a side effect on disk. What decides is
/// [`packaging`].
const SETTINGS: &str = "settings";

/// The one switch a launch takes: `acter --connect <name>` (decision 20).
const CONNECT: &str = "--connect";

/// The name of the thing this product writes its settings under, inside whatever folder the
/// operating system keeps an account's configuration in.
const ACTER: &str = "acter";

/// The two things every router reaches: the session that is live, and the actions that
/// change which one that is.
///
/// **Both are the same object.** `ConnectService` owns which session is current, so it is
/// what can answer a submitted line *and* what can replace the thing answering it. Holding
/// them as two typed handles rather than one keeps each router depending on the port it
/// actually uses.
pub(crate) struct AppState {
    pub(crate) session: Arc<dyn SessionApi>,
    pub(crate) connect: Arc<dyn ConnectApi>,
    /// Attempts to connect, which are neither of the above: an attempt is not a session
    /// yet, and it is not a question about what this machine offers. It is a conversation
    /// in flight (spec B9).
    pub(crate) connecting: Arc<Connecting>,
    /// Everything Acter writes, and the two build facts a listener can be told (spec 26,
    /// decision 10).
    ///
    /// **The About router holds the object itself** rather than a copy of its answers,
    /// because the runtime values are the only thing it wants — and it must not ask the
    /// environment a second time, since a second lookup could name a folder the files are
    /// not in.
    pub(crate) settings: Arc<Settings>,
}

/// Where Acter keeps everything it writes, and how it came to be there.
///
/// **A value in the composition root rather than a domain type**, because it is an answer
/// about *this machine and this build*. What the domain gets is a path, and what the About
/// dialog gets is these two facts (decision 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsFolder {
    pub(crate) path: PathBuf,
    pub(crate) standing: Standing,
}

/// Why the settings folder is where it is — the second half of the line About reads out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// A development build, which keeps its settings where it was started from so a
    /// developer needs no variable set to get a sane answer.
    Development,
    /// This copy was packaged portable, so its settings are beside the program.
    Portable,
    /// This copy was packaged for the installer, so its settings are with the rest of this
    /// account's configuration.
    Installed,
    /// [`SETTINGS_DIR`] named a folder, which wins over the packaging either way.
    Directed,
    /// Nowhere else to put them: an operating system that reports no configuration
    /// directory at all, or a portable copy that cannot tell where its own program is.
    /// Acter writes where it was started from, which is a real answer rather than a
    /// failure — but it is a different one, and About says so.
    WhereItStarted,
}

impl Standing {
    /// What the About dialog says about it, as a whole sentence: this is read aloud, and
    /// "portable" on its own is a word with no verb (CLAUDE.md).
    pub(crate) fn said(self) -> &'static str {
        match self {
            Self::Development => {
                "This is a development build of Acter, so its settings are kept in the \
                 folder it was started from."
            }
            Self::Portable => {
                "Acter is running portable, so its settings are kept beside the program."
            }
            Self::Installed => {
                "Acter is installed, so its settings are kept with your other application \
                 data."
            }
            Self::Directed => "Acter was told where to keep its settings.",
            Self::WhereItStarted => {
                "Acter has nowhere of its own to keep its settings on this system, so it \
                 uses the folder it was started from."
            }
        }
    }
}

/// How this copy of Acter was packaged: the whole of the portable question, decided when
/// the binary was built rather than guessed at when it runs (decision 3).
///
/// **The packaging is the fact, so the package is what says it.** An installer put a copy
/// somewhere and a zip did not, and nothing a running program can see on disk distinguishes
/// those two reliably — a marker beside the executable can be created by accident, copied
/// into place, or left behind by an unzip nobody meant to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packaging {
    /// Any debug build. **A case of its own** so a developer needs no variable set to get a
    /// sane answer, which is the whole reason it exists.
    Development,
    /// Built for the portable zip: `cargo build --release --features portable`.
    Portable,
    /// A release without the feature, which is what the installer ships.
    Installed,
}

/// What this build is: the identifier a bug report carries, and the sentence About speaks
/// (decision 5).
///
/// **Two strings rather than one**, because they are for two different people.
/// `development-521c956` is a value and not a sentence: a listener hears "Development
/// build, commit 521c956", and the raw identifier stays here so a bug report can still
/// carry it — the About dialog is copyable text, and nothing tries to spell a commit out
/// loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    /// What a bug report carries: `1.0.0`, or `development-521c956`.
    pub(crate) identifier: String,
    /// What About reads out, as a whole sentence.
    pub(crate) said: String,
}

impl Version {
    /// A release, named by its tag's numeric triple and nothing else.
    ///
    /// **The platform is not in it** (decision 3). Tags are `<platform>-vx.y.z`, so
    /// `windows-v1.0.0` and `macos-v1.0.0` are the same release of the same product for two
    /// machines; a listener hears "Version 1.0.0" on either, because the platform is a fact
    /// about which file they downloaded rather than about which Acter they are running.
    pub(crate) fn released(number: &str) -> Self {
        Self {
            identifier: number.to_owned(),
            said: format!("Version {number}."),
        }
    }

    /// Anything with a commit behind it that is not a release.
    pub(crate) fn development(commit: &str) -> Self {
        Self {
            identifier: format!("development-{commit}"),
            said: format!("Development build, commit {commit}."),
        }
    }

    /// A source tree with no git at all, which is what a source tarball is.
    fn from_cargo(number: &str) -> Self {
        Self::released(number)
    }
}

pub fn run() {
    // A session starts two tasks and arms a timer, so one is built inside the runtime
    // Tauri owns rather than beside it. Entering that runtime is the composition root's
    // business by definition: it is the one place that knows a runtime exists at all —
    // and since B7 the factory enters it too, because a session can now be started long
    // after this function has returned, from whichever thread answers an invoke.
    let runtime = tauri::async_runtime::handle();
    let state = {
        let _entered = runtime.inner().enter();
        connected_state()
    };

    // The operating system's own menu bar, for an operating system that has one Acter puts
    // anything in (spec M3). On Windows this hands the builder straight back, so the
    // platform where a native menu freezes NVDA for tens of seconds keeps the menu bar A7
    // put in the document — and macOS stops getting Tauri's default menu, whose Help
    // submenu is empty and whose File menu has no Connect in it.
    let builder = install_system_menu(Builder::default(), consts::OS)
        .manage(state)
        .invoke_handler(generate_handler![
            crate::routers::attach_session,
            crate::routers::submit_command,
            crate::routers::send_key,
            crate::routers::set_line_owner,
            crate::routers::paste,
            crate::routers::about,
            crate::routers::platform,
            crate::routers::connectable,
            crate::routers::use_profile,
            crate::routers::answer_connect,
            crate::routers::attempt_ended,
            crate::routers::connected,
            crate::routers::saved,
            crate::routers::save_connection,
            crate::routers::rename_connection,
            crate::routers::forget_connection,
            crate::routers::offer_to_save,
            crate::routers::stop_offering_to_save,
            crate::routers::requested_at_launch
        ]);

    // Embedded WebDriver server for E2E tests (spec T2): debug builds only, so
    // release binaries carry no automation surface. Debug builds exist only on
    // developer machines and CI.
    #[cfg(debug_assertions)]
    let builder = {
        use tauri_plugin_wdio_webdriver::init;
        builder.plugin(init())
    };

    // The frontend's debug event recorder, gated the same way and for the same reason
    // (spec A3.2, decision 12). This injects one flag before any page script runs; the
    // frontend reads it and, only then, wraps its backend and installs the reader at
    // `window.__acterDebug`. A release build injects nothing, so the recorder is not
    // merely disabled there — it is never constructed.
    #[cfg(debug_assertions)]
    let builder = builder.plugin(
        tauri::plugin::Builder::<tauri::Wry>::new("acter-debug")
            .js_init_script("window.__ACTER_DEBUG__ = true;".to_owned())
            .build(),
    );

    builder
        .run(generate_context!())
        .expect("failed to start the Acter window");
}

/// The managed state a launch produces: the connect service, plus whatever session the
/// environment asked for.
///
/// **A shell the environment named and could not start is no longer a panic, and the
/// window opens anyway.** It used to take the whole application down, which was defensible
/// while the only way to name a shell was a variable a developer set. Since B7 there is a
/// window that can say what went wrong, and a window that says so is more use than a
/// process that vanished — most of all to somebody who cannot see that it vanished. The
/// sentence reaches the user the way every other connection failure does: the window opens
/// unconnected, and the reason is the first thing it says.
pub(crate) fn connected_state() -> AppState {
    // **Resolved once, here, and handed to everything that needs it** (spec 26,
    // decision 10). The folder is one answer about this machine, and a second lookup could
    // name a folder the files are not in — which is also why the About router is given this
    // object rather than the environment.
    let settings = settings();
    let service = Arc::new(state(&settings));
    if let Some(profile) = launch_profile() {
        // Nothing reads the error here on purpose: `connected()` answers `None`, which is
        // the unconnected window, and the frontend says so. Reporting the reason as well is
        // B8's, where a `--profile` that resolves to nothing has to name what was asked for.
        // Nobody to ask at launch: there is no window yet, so a far end that needs a
        // decision is refused rather than trusted (`Unasked`).
        // **Set up by nobody, because there is nobody to ask** (spec B9.5, decision 9): the
        // checkbox authorises and the dialog discloses, and a launch that names a profile from
        // the environment has neither. `Unasked` refuses, so such a session runs and is told
        // nothing — which is what it does for a host key and for an unverified file already.
        // Started from no saved connection, whatever the environment named: `--connect`
        // is the switch that names one, and it is answered by the window rather than here
        // (spec 26, decision 20).
        let _ = service.use_profile(
            &profile,
            SetUp::Yes,
            None,
            &(Arc::new(Unasked) as Arc<dyn ConnectQuestions>),
        );
    }
    let connect = Arc::clone(&service) as Arc<dyn ConnectApi>;
    AppState {
        session: Arc::clone(&service) as Arc<dyn SessionApi>,
        connecting: Arc::new(Connecting::new(Arc::clone(&connect))),
        connect,
        settings,
    }
}

/// The settings object, resolved from the world once (spec 26, decision 10).
///
/// **This is the one place the world is read for it**: the variable, the packaging, where
/// the program is, where this operating system keeps an account's configuration, and what
/// the build stamped. Everything above takes the answers as values, which is what lets one
/// machine assert both platforms and all three packagings.
pub(crate) fn settings() -> Arc<Settings> {
    Arc::new(Settings::open(settings_directory(), stamped()))
}

/// The connect service, wired and empty — the shape every test of the invoke surface wants.
///
/// **The settings object is passed in rather than built here**, which is the whole of
/// decision 10: one object owns the folder, and the store the service talks to is that same
/// object seen through the port it needs.
pub(crate) fn state(settings: &Arc<Settings>) -> ConnectService {
    ConnectService::new(
        Arc::new(Shells::new(settings)),
        machine(),
        signatures(),
        offered(consts::OS).to_vec(),
        scripted_profiles(),
        Arc::clone(settings) as Arc<dyn ConnectionStore>,
    )
    .asked_for(requested_connection(env::args_os().skip(1)))
}

/// The connect service, as a named platform would build it — the shape a test that is about
/// the list itself wants.
///
/// **`state()` is this with the operating system it was compiled for**, which is the one
/// place in the product where "which platform is this" is read. Nothing else needs the seam;
/// the tests below do, because a list is the thing about connecting that differs by platform
/// and asserting it only on the machine that happens to run CI is how M1's third failure got
/// in (spec M1, decision 1).
#[cfg(test)]
fn state_on(os: &str) -> ConnectService {
    let settings = settings();
    ConnectService::new(
        Arc::new(Shells::new(&settings)),
        machine(),
        signatures(),
        offered(os).to_vec(),
        scripted_profiles(),
        settings as Arc<dyn ConnectionStore>,
    )
}

/// Who signed the files this machine would start (spec B5.7).
///
/// **The platform choice is made here and nowhere else**, which is what the composition root
/// is for. Windows has `WinVerifyTrust`, a catalog database and a certificate store; nothing
/// else this product runs on has any of the three, and acter-core's `Unchecked` is what
/// stands in — it vouches for nothing, so a build with no way to check asks the user rather
/// than assuming (spec B5.7, decision 6).
#[cfg(windows)]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(WindowsTrust::new())
}

/// macOS has the Security framework, and M2 is where it arrives — in the same entry that
/// gives this platform a local file to start, rather than in a later one.
///
/// **The order is the whole reason** (DESIGN, decided 2026-08-31). `Unchecked` vouches for
/// nothing, which is the right refusal and the wrong thing to ship beside a Terminal row: a
/// security dialog on every connection to a `/bin/zsh` Apple signed is a dialog a listener
/// learns to dismiss, and it is the one dialog in this product that is about security.
#[cfg(target_os = "macos")]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(AppleTrust::new())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(acter_core::Unchecked)
}

/// What this computer has, for the connect list and for the one question a connection asks
/// it (spec M2, decision 1).
///
/// **The second gated function in this file, and it is the same shape as
/// [`signatures`]** — which is ARCHITECTURE's platform-divergence rule taken at its word: a
/// whole module that would have been `#[cfg]`-gated is an adapter, and the composition root
/// is where one of two adapters is chosen.
///
/// **Off Windows is Unix rather than macOS.** `/etc/shells` and a passwd entry are POSIX, so
/// a Linux build gets a correct answer from this arm rather than a stub — what it will not
/// get until somebody does the work is a `Terminal` kind in its catalogue, which is
/// `offered`'s answer and not this one's.
#[cfg(windows)]
fn machine() -> Arc<dyn ThisComputer> {
    Arc::new(WindowsMachine::new())
}

#[cfg(not(windows))]
fn machine() -> Arc<dyn ThisComputer> {
    Arc::new(UnixMachine::new())
}

/// Which profile this launch asked for, or `None` for a window that starts unconnected.
///
/// `ACTER_SHELL` wins over `ACTER_TRANSCRIPT` because a developer who has set both has
/// asked for a real shell most recently; before B7 the same precedence was a `match` on the
/// first variable with the second only reachable through its `Err` arm.
fn launch_profile() -> Option<ProfileId> {
    if let Ok(program) = env::var(SHELL_ENV) {
        return Some(ProfileId::Program { program });
    }
    env::var(TRANSCRIPT_ENV)
        .ok()
        .map(|name| ProfileId::Scripted { name })
}

/// The scripted sessions this build offers.
///
/// **The debug gate, at the list** (spec B7, decision 7). DESIGN has said since A3 that the
/// scripted fake is a permanent, selectable session kind rather than a launch-time
/// environment variable, and this is where it becomes selectable. A release build offers
/// none — it does not hide them, it never names them — exactly as T2 gated the embedded
/// WebDriver.
///
/// A path to a transcript file is still a scripted profile and is still startable in a
/// debug build; what this decides is only what the connect *list* offers without one being
/// typed.
#[cfg(debug_assertions)]
fn scripted_profiles() -> Vec<String> {
    ["builtin", "builtin-by-byte", "unmarked", "unmarked-by-byte"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(not(debug_assertions))]
fn scripted_profiles() -> Vec<String> {
    Vec::new()
}

/// Adapter: the [`SessionFactory`] — the one thing that knows a transport, an engine, a
/// clock and a `SessionService` belong to each other.
///
/// It lives in the composition root because building one means naming four concrete
/// implementations, and ARCHITECTURE allows exactly one place to do that. Everything above
/// it — what is offered, what replacing means, what happens when a shell will not start —
/// is `ConnectService`'s, and is tested with a fake in place of this.
struct Shells {
    clock: Arc<dyn Clock>,
    /// What this computer has, for the one question a *connection* asks it: which shell the
    /// distribution being connected to actually runs (spec B5.5, decision 2).
    ///
    /// The same port the connect list is built from, and the same instance would do — it
    /// holds nothing and caches nothing. It is here rather than passed in because this is
    /// the one branch that needs an answer before it can decide what to inject, and that
    /// decision has to be made before the client is started at all.
    machine: Arc<dyn ThisComputer>,
    /// Which shells this person has already had Acter's setup command explained to them for
    /// (spec B9.5, decision 10).
    ///
    /// It is here rather than in `ConnectService` because this is where the question is asked:
    /// the dialog names the shell it detected, and which shell that is is not known until the
    /// far end has answered — which happens inside this factory.
    explained: Arc<dyn Explained>,
    /// Acter's own record of the servers this person accepted (spec B9, decision 5).
    ///
    /// **The settings object behind the port it fills** (spec 26, decision 2 as amended,
    /// and decision 10). It was a path on disk resolved on every SSH attempt, from a
    /// function that read the environment; it is a typed list in the one document now, and
    /// what the transport is handed is the seam rather than a file name.
    known_hosts: Arc<dyn HostKeyStore>,
}

impl Shells {
    fn new(settings: &Arc<Settings>) -> Self {
        Self {
            clock: Arc::new(SystemClock::new()),
            machine: machine(),
            explained: Arc::new(ExplainedShells::new(settings.explained_shells())),
            known_hosts: Arc::clone(settings) as Arc<dyn HostKeyStore>,
        }
    }

    /// One session over a far end, whatever that far end is.
    fn session(
        &self,
        transport: Box<dyn Transport>,
        shell: &dyn ShellAdapter,
    ) -> Arc<dyn SessionApi> {
        self.session_with(transport, ShellFacts::of(shell))
    }

    /// The same, for a far end nothing on this machine started.
    ///
    /// **An SSH session has shell facts without having a shell adapter**, and that is not a
    /// gap: an adapter knows how to *launch* a shell and what to inject into it, and neither
    /// applies to a program `sshd` chose from an account's passwd entry on another machine.
    /// What is known is what the far end said it was (spec B9, decision 7).
    fn session_with(
        &self,
        transport: Box<dyn Transport>,
        facts: ShellFacts,
    ) -> Arc<dyn SessionApi> {
        Arc::new(SessionService::start(
            transport,
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::clone(&self.clock),
            PacingConfig::default(),
            facts,
        ))
    }

    /// A far end that is not on this machine.
    ///
    /// **Connecting is async and this is not, so the work is handed to the runtime and
    /// waited for on a channel.** Blocking here is correct rather than merely tolerable:
    /// this runs on the blocking task the connect controller spawned precisely so that a
    /// person can be asked things, and no invoke is waiting on it (spec B9).
    fn ssh(
        &self,
        host: &str,
        port: u16,
        user: &str,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        let target = SshTarget {
            host: host.to_owned(),
            port,
            user: user.to_owned(),
        };
        let hosts = Arc::new(KnownHosts::new(
            Arc::clone(&self.known_hosts),
            users_known_hosts(),
        ));
        // The SSH half of the same asker: the transport must not be handed a question it can
        // never ask, and it is measured against a real server with no window anywhere near it.
        let asker = Arc::clone(questions) as Arc<dyn SshQuestions>;
        let (done, waiting) = std::sync::mpsc::channel();
        tauri::async_runtime::spawn(async move {
            let _ = done.send(
                SshTransport::connect(
                    &target,
                    hosts,
                    asker,
                    COLUMNS,
                    SCREEN_LINES,
                    probe_patience(),
                )
                .await,
            );
        });
        let transport = waiting
            .recv()
            .map_err(|_| "The connection stopped before it could be made.".to_owned())??;

        // **The name is what decides the setup, and the setup is what decides the markers**
        // (spec B9.5, decision 2). Knowing a far end is bash still does not make bash emit
        // anything — what does is the line Acter sends into the session once it is up, which
        // is the same line a WSL bash gets, because a setup is a property of the shell rather
        // than of how Acter reached it.
        let name = transport.far_end().name();
        let (facts, outcome) = self.agreed(
            acter_shells::over_ssh(name.as_deref()),
            name.as_deref(),
            set_up,
            questions,
        );
        let note = far_end_note(name.as_deref(), outcome);
        Ok(Started {
            session: self.session_with(Box::new(transport), facts),
            note: note.said,
            limit_explained: note.limit_explained,
        })
    }

    /// Whether this session may be set up, and what to run inside it if it may.
    ///
    /// **Asked here because this is the window the question belongs in** (spec B9.5,
    /// decision 9): the connection has succeeded, the far end has said what shell it runs, and
    /// nothing has been sent yet. It is asked *at all* only because the Connect dialog's
    /// checkbox said so, and asked *of a person* only when this shell has not been explained
    /// to them before — which is `Explained`'s, kept per shell and by nothing else.
    ///
    /// The two refusals are one outcome, deliberately: a listener is owed the same sentence
    /// whether they unticked a box a minute ago or cancelled a dialog a second ago, because
    /// what it says is what this session will and will not be able to tell them.
    fn agreed(
        &self,
        facts: ShellFacts,
        shell: Option<&str>,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> (ShellFacts, SetUpOutcome) {
        // A far end that answered nothing, or one running a shell nobody has measured a setup
        // for. Both start, and neither is experimented on.
        let (Some(setup), Some(shell)) = (facts.setup.clone(), shell) else {
            return (facts, SetUpOutcome::NothingWritten);
        };
        if !set_up.wanted() {
            return (facts.declined(), SetUpOutcome::Refused);
        }
        if !self.explained.already(shell) {
            let asked = SetupQuestion {
                shell: shell.to_owned(),
                setup,
            };
            match questions.set_up_session(asked) {
                SetupAnswer::SetUp { remember } if remember => self.explained.remember(shell),
                SetupAnswer::SetUp { .. } => {}
                SetupAnswer::Skip => return (facts.declined(), SetUpOutcome::Refused),
            }
        }
        // **What the sentence has to be careful about is the verdict** (roadmap 23.15): a
        // shell that says how a command went gets the full sentence whether or not it also
        // marks where output begins, because the tracker supplies that boundary itself.
        let outcome = if facts.markers.reports_exit_code() {
            SetUpOutcome::Fully
        } else {
            SetUpOutcome::Partly
        };
        (facts, outcome)
    }

    /// A far end this machine starts itself, named by the program that is it.
    ///
    /// **One branch rather than three, because WSL has to be told apart before it is
    /// started** (spec B5.5). Every other local shell is fully described by its name —
    /// `adapter_for` recognises it and knows what to inject — where `wsl.exe` is a client
    /// whose far end has to be asked about first. `ConnectionKind::Wsl` and
    /// `ACTER_SHELL=wsl` both arrive here and both mean the distribution WSL calls the
    /// default, which is a real session and the one `wsl.exe` with no arguments opens.
    fn local(
        &self,
        program: &str,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        match is_wsl(program) {
            true => self.wsl(program, None, set_up, questions),
            false => self.real(adapter_for(program)).map(only),
        }
    }

    /// Whatever shell one WSL distribution's account actually runs.
    ///
    /// **The probe runs beside the launch rather than in front of it** (spec B9.5,
    /// decision 14). Until this entry, what was injected was part of `ShellLaunch`, so the
    /// decision whether to inject could not be made after the client had started and the
    /// probe's whole wait landed in front of the user's first byte. Nothing is injected now,
    /// so the two are started together and the client's boot and the probe's answer overlap —
    /// which on a cold distribution is the difference between waiting for one boot and waiting
    /// for two things that each wait for it.
    ///
    /// **The answer still has to arrive before the session does**, and that is why the probe
    /// did not leave the critical path entirely: the dialog names the shell it detected, and
    /// `ShellFacts` is a construction argument, so a narrower marker claim for `sh` is known
    /// before the first byte is tracked and nothing has to mutate the tracker mid-session.
    ///
    /// **The answer decides two things and no more**: what is run inside the session, and what
    /// the connection sentence says. It never decides whether the session starts.
    fn wsl(
        &self,
        client: &str,
        distribution: Option<&str>,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        // **Said before the wait, not after it** — the one thing SSH does here that the
        // local branches never needed to. `LocalPty` spawns a process in milliseconds, so
        // there was nothing to say; a cold distribution takes five to six seconds to boot
        // and this call is what boots it, which is long enough that silence is
        // indistinguishable from a program that has stopped. `tell` is B9's own mechanism
        // and reaches a listener as `ConnectStep::Progress`, exactly as "Connecting to
        // acter-ssh." does.
        questions.tell(&starting(distribution));

        // Asked on a thread of its own so the client can be started while the distribution is
        // still working out what it runs. Both wait on the same cold boot, so what is saved is
        // the whole of one of them.
        let machine = Arc::clone(&self.machine);
        let named = distribution.map(ToOwned::to_owned);
        let asking = std::thread::spawn(move || machine.login_shell(named.as_deref()));

        let adapter = match distribution {
            Some(name) => Wsl::in_distribution(client, name, None),
            None => Wsl::new(client, None),
        };
        let pty = self.pty(&adapter.launch())?;
        // A probe whose thread died is a probe that answered nothing, which is a state this
        // product supports: the session starts, and nothing is claimed about it.
        let shell = asking.join().unwrap_or_default();
        let adapter = adapter.running(shell.as_deref());

        let (facts, outcome) = self.agreed(
            ShellFacts::of(&adapter),
            adapter.login_shell(),
            set_up,
            questions,
        );
        let note = far_end_note(adapter.login_shell(), outcome);
        Ok(Started {
            session: self.session_with(Box::new(pty), facts),
            note: note.said,
            limit_explained: note.limit_explained,
        })
    }

    /// A shell on this Unix machine, started as a login shell.
    ///
    /// **It goes through [`Self::agreed`] where [`Self::local`] does not**, and that is the
    /// difference the Terminal row is about: a local Windows shell is set up by what its
    /// adapter injects, while this one is set up by a line sent into the session after it is
    /// up — which is the mechanism B9.5 built for WSL and SSH, reaching a third transport
    /// unchanged. So the same question is asked, the same checkbox authorises it, and the
    /// same sentence is composed about what the session will be able to tell the listener.
    fn unix_shell(
        &self,
        program: &str,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        let adapter = UnixShell::new(program);
        let pty = self.pty(&adapter.launch())?;
        let shell = Path::new(program)
            .file_name()
            .map(|file| file.to_string_lossy().into_owned());
        let (facts, outcome) = self.agreed(
            ShellFacts::of(&adapter),
            shell.as_deref(),
            set_up,
            questions,
        );
        let note = far_end_note(shell.as_deref(), outcome);
        Ok(Started {
            session: self.session_with(Box::new(pty), facts),
            note: note.said,
            limit_explained: note.limit_explained,
        })
    }

    /// A real shell on a pseudoconsole.
    ///
    /// The adapter and the far end are one decision, which is why they travel together:
    /// injecting cmd's prompt markers without telling the domain that this shell emits no
    /// `C` produces a session that receives markers, opens no block and speaks nothing at
    /// all — measured before either half was written (ROADMAP 22.5).
    fn real(&self, shell: Box<dyn ShellAdapter>) -> Result<Arc<dyn SessionApi>, String> {
        let pty = self.pty(&shell.launch())?;
        Ok(self.session(Box::new(pty), shell.as_ref()))
    }

    /// The pseudoconsole one launch describes.
    ///
    /// Separate from [`Self::real`] since B9.5, for the one branch that starts the client
    /// before it knows what shell is behind it: the launch no longer depends on that answer,
    /// so the process can be spawned while the probe is still being answered.
    fn pty(&self, launch: &ShellLaunch) -> Result<LocalPty, String> {
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        let environment: Vec<(&str, &str)> = launch
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
    }

    /// A scripted far end: a shell Acter knows nothing about, which is what [`Plain`] is.
    /// The transcript speaks the markers itself, and nothing is injected into a fake.
    #[cfg(debug_assertions)]
    fn scripted(&self, name: &str) -> Result<Arc<dyn SessionApi>, String> {
        let (shell, chunking) = far_end(name)?;
        let transport = ScriptedTransport::with_shell(shell, chunking, Arc::clone(&self.clock));
        Ok(self.session(Box::new(transport), &Plain::new(SCRIPTED)))
    }
}

impl SessionFactory for Shells {
    /// **Every branch enters the Tauri runtime first.** A session spawns two tasks and arms
    /// a timer, and `use_profile` is called from whichever thread Tauri answers an invoke
    /// on — which is not inside that runtime. Doing it here rather than at each call site is
    /// the same reasoning that puts the rest of this file in the composition root: knowing a
    /// runtime exists at all is its privilege.
    fn open(
        &self,
        chosen: &Chosen,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        let runtime = tauri::async_runtime::handle();
        let _entered = runtime.inner().enter();
        self.start(chosen, set_up, questions).map_err(ended)
    }
}

impl Shells {
    /// The branch per kind of profile, with the runtime already entered.
    ///
    /// **Every local branch starts the file it was handed rather than the name it was
    /// chosen by** (spec B5.7, decision 1). The connect service resolved that file once and
    /// verified it; resolving the name again here is what would let Windows land on a
    /// different one, which is the whole thing this entry removes.
    fn start(
        &self,
        chosen: &Chosen,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        let program = chosen.program.as_deref().map(Path::to_string_lossy);
        let program = program.as_deref();
        match &chosen.profile {
            ProfileId::Ssh { host, port, user } => self.ssh(host, *port, user, set_up, questions),
            // **A Terminal profile names a file rather than a kind**, and what starts it is
            // the adapter that knows a Unix login shell: `-l`, a setup keyed by the file's
            // own name, and an ending somebody measured (spec M2, decision 4). `adapter_for`
            // would answer `Plain` here — a shell started as it stands, with nothing run
            // inside it — which is the state this row exists to be better than.
            ProfileId::Shell {
                kind: ConnectionKind::Terminal,
            }
            | ProfileId::Install {
                kind: ConnectionKind::Terminal,
                ..
            } => {
                let program = program.ok_or_else(|| {
                    "Acter could not work out which shell to start on this Mac.".to_owned()
                })?;
                self.unix_shell(program, set_up, questions)
            }
            // The composition root names no shell since B5.1: which shell this is, what it
            // is started with and what it can mark are one object's answers, and the same
            // object answers them for the suites that measure a real one. **Which shell it
            // is stays the kind's answer and where it is stays the file's**: an adapter
            // selected by the resolved path is the same adapter, because `adapter_for`
            // recognises a full path as readily as a bare name.
            ProfileId::Shell { kind } => {
                self.local(program.unwrap_or_else(|| kind.program()), set_up, questions)
            }
            ProfileId::Install { kind, .. } => {
                self.local(program.unwrap_or_else(|| kind.program()), set_up, questions)
            }
            ProfileId::Distribution { name } => {
                self.wsl(program.unwrap_or(WSL_CLIENT), Some(name), set_up, questions)
            }
            // Trimmed for the reason A9 found: `set ACTER_SHELL=x && acter` in cmd puts
            // everything up to the `&&` into the value, trailing space included — and a
            // name with a stray space matches no adapter, so the session silently became an
            // unintegrated shell with no injection, no markers and no prompt. A shell nobody
            // recognises is a real state this product supports; being pushed into it by
            // punctuation is not. The trimming now happens where the name is resolved, and
            // this falls back to it for a name nothing resolved.
            ProfileId::Program { program: named } => {
                self.local(program.unwrap_or_else(|| named.trim()), set_up, questions)
            }
            #[cfg(debug_assertions)]
            ProfileId::Scripted { name } => self.scripted(name).map(only),
            // **The gate, at the factory.** A release build does not refuse to construct a
            // scripted session at run time by checking a flag — this arm does not exist in
            // it, and neither does the code it would have called.
            #[cfg(not(debug_assertions))]
            ProfileId::Scripted { name } => Err(format!(
                "The scripted session {name} is only available in a development build of \
                 Acter."
            )),
        }
    }
}

/// A session with nothing to say about the far end it reached, which is every far end that
/// is a program on this machine: the row the user chose already described it.
fn only(session: Arc<dyn SessionApi>) -> Started {
    Started {
        session,
        note: None,
        limit_explained: false,
    }
}

/// What a listener hears while a distribution is coming up, which on a cold one is several
/// seconds of otherwise complete silence.
///
/// **It names the distribution when there is a name to say.** A user who chose "Ubuntu 24.04"
/// from the list hears the words they chose, which is A9's rule about the window applied to
/// the wait before there is one. A launch that named no distribution has nothing truthful to
/// name — asking WSL for its default is deliberately not the same as this program deciding
/// which one that is (spec B5.3) — so it says what is happening without inventing a subject.
///
/// **A statement, not a stage.** SSH says three things because it has three stages a listener
/// can be at; this has one thing to wait for, and saying "asking the distribution what shell
/// it runs" would describe Acter's internals rather than the user's wait.
fn starting(distribution: Option<&str>) -> String {
    match distribution {
        Some(name) => format!("Starting {name}."),
        None => "Starting Linux.".to_owned(),
    }
}

/// What became of setting this session up, which is what the connection sentence has to say
/// (spec B9.5, decision 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetUpOutcome {
    /// The setup went out, and what it earns reaches every boundary.
    Fully,
    /// The setup went out, and it marks where commands begin but not how they ended — the
    /// only case where the grace period will never speak, because markers do arrive.
    ///
    /// **Nothing shipped reaches it since roadmap 23.15**, and it is kept rather than deleted.
    /// It was POSIX `sh` until `sh` was measured to report exit codes after all, and `cmd.exe`
    /// — the one remaining shell that marks only its prompt — is injected at launch and never
    /// reaches this question. What it is waiting for is the next shell somebody measures whose
    /// prompt is all it has: `fish` has a hook of its own, and zsh — measured in B5.8 — turned
    /// out to reach the whole cycle. A shell with neither hook is not a hypothesis, it is where
    /// `sh` was believed to be a day ago. Deleting the state would mean rebuilding it, and
    /// rebuilding the sentence a listener hears with it.
    Partly,
    /// Nothing has been measured for this shell, so there was nothing to run.
    NothingWritten,
    /// There is a setup and it was not run: the Connect dialog's checkbox was unticked, or its
    /// dialog was cancelled.
    ///
    /// **One outcome for both refusals**, because a listener is owed the same sentence either
    /// way: what it says is what this session will and will not be able to tell them, not
    /// which control they used to say no.
    Refused,
}

/// What a connection adds to what a listener hears, said once, and whether it has already
/// covered what the grace period would otherwise say.
///
/// **Both come out of one function**, which is what keeps the status region and the
/// announcement the same string (roadmap 27.5) and what stops the sentence and the fact about
/// it from drifting apart.
struct Note {
    said: Option<String>,
    limit_explained: bool,
}

/// What a connection adds to what a listener hears, said once (spec B9.5, decision 13, which
/// rewrites B9's decision 7 and B5.5's decision 5 and closes roadmap 13.8).
///
/// **The phrase A13 removed is gone from all of it.** A connection used to announce "bash,
/// with no shell integration set up on this host", and "shell integration" is exactly the
/// vocabulary A13 concluded a user does not have — said as the *first* thing a listener hears
/// on connecting, so it met the objection before the announcement A13 rewrote did. This entry
/// writes clauses for two new states anyway, so it rewrites all of them rather than leaving
/// the product speaking in two registers.
///
/// **Four cases, and each says what it knows.** Set up fully: the name and nothing more,
/// because nothing is missing. Set up partly: the name, and what a listener will and will not
/// get. Named with nothing written for it: the reason B5.5 currently only implies. Nothing
/// answered: no clause at all — the name is not invented, and the sentence that covers this
/// case is A13's, said when the grace period expires. Saying it twice in two registers is what
/// 13.8 objected to.
///
/// **A fifth case the spec's decision 13 did not enumerate**, and it is the one the checkbox
/// and the dialog create: a shell that has a setup, refused. It says the name and then A13's
/// own sentence, which is the same words the help topic F1 opens uses — recorded as an
/// amendment in the spec.
///
/// **The place is gone**, and that is deliberate rather than lost. "on this host" and "in this
/// distribution" existed to point at the dotfile a user would have to edit; there is no
/// dotfile to edit any more, and every one of these sentences is now about *this session*.
fn far_end_note(shell: Option<&str>, outcome: SetUpOutcome) -> Note {
    let Some(shell) = shell else {
        return Note {
            said: None,
            limit_explained: false,
        };
    };
    match outcome {
        SetUpOutcome::Fully => Note {
            said: Some(shell.to_owned()),
            limit_explained: false,
        },
        // **Not "a heading for each command", which it said until 2026-08-30** — reported by
        // the user, who read the same claim in the set-up dialog and asked whether it was
        // true. It is not: a session gets a heading for every line it is given, set up or
        // not (B4.4, and measured with NVDA the same day). What a prompt-only setup buys is
        // that Acter knows when a command has finished, which is why the prompt is read on
        // its own rather than at the end of the output.
        SetUpOutcome::Partly => Note {
            said: Some(format!(
                "{shell}. Acter can tell when a command has finished here, but not whether \
                 it worked."
            )),
            limit_explained: true,
        },
        SetUpOutcome::NothingWritten => Note {
            said: Some(format!("{shell}, which Acter cannot set up yet.")),
            limit_explained: true,
        },
        SetUpOutcome::Refused => Note {
            said: Some(format!(
                "{shell}. You will hear what commands print here, but not whether they worked."
            )),
            limit_explained: true,
        },
    }
}

/// The settings folder, resolved from the world (spec 26, decisions 2 and 3).
///
/// **The world is read here and the rule is [`settings_folder`], which is pure** — so both
/// platforms and all three packagings are asserted from whichever machine the suite happens
/// to run on, which is what ARCHITECTURE's platform-divergence rule asks for and what M1
/// exists because of.
pub(crate) fn settings_directory() -> SettingsFolder {
    settings_folder(
        consts::OS,
        packaging(),
        // **Asked of the operating system rather than of `%APPDATA%`** (decision 3). On
        // Windows this is the known-folder API, where reading the variable trusts something
        // that can be missing or redirected — and it collapses the per-platform branch to
        // nothing, because both platforms answer their own base and both then join the same
        // suffix.
        dirs::config_dir().as_deref(),
        // Where the running program is, which a portable copy needs and nothing else does.
        // Nothing is asked of the filesystem: this is a path the operating system already
        // knows, and whether anything exists at it is nobody's question here.
        program_directory().as_deref(),
        env::current_dir().ok().as_deref(),
        env::var_os(SETTINGS_DIR)
            .filter(|named| !named.is_empty())
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// The directory the running program is in, or `None` on a machine that will not say.
fn program_directory() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(Path::to_path_buf))
}

/// How this build was packaged (decision 3).
///
/// **One `cfg!` expression rather than a gated pair of functions**, which is
/// ARCHITECTURE's platform-divergence rule taken at its word: prefer no gate at all where
/// the answer is a value. `cfg!` is a compile-time boolean, so this costs nothing at run
/// time and **both arms are still compiled** — where a `#[cfg]` on the function would leave
/// the variants this build is not one of unconstructed, which clippy fails the portable
/// build over as dead code.
///
/// **The feature wins over the debug default**, so a portable build can be made and driven
/// on a developer's machine.
fn packaging() -> Packaging {
    if cfg!(feature = "portable") {
        Packaging::Portable
    } else if cfg!(debug_assertions) {
        Packaging::Development
    } else {
        Packaging::Installed
    }
}

/// Where Acter keeps its settings, given how this copy was packaged, what the operating
/// system said, where the program is, and where it was started from.
///
/// **Pure, and the packaging arrives as a value** (decision 4), so an ordinary `cargo test`
/// covers every branch whichever packaging is being compiled.
///
/// The order is: the variable, then the packaging.
fn settings_folder(
    os: &str,
    packaging: Packaging,
    configuration: Option<&Path>,
    program: Option<&Path>,
    working: Option<&Path>,
    directed: Option<&Path>,
) -> SettingsFolder {
    if let Some(directed) = directed {
        return SettingsFolder {
            path: directed.to_path_buf(),
            standing: Standing::Directed,
        };
    }
    match packaging {
        // A developer needs no variable set to get a sane answer, which is the whole reason
        // this case exists.
        Packaging::Development => where_it_started(working, Standing::Development),
        Packaging::Portable => match program {
            Some(beside) => SettingsFolder {
                path: portable_settings(os, beside),
                standing: Standing::Portable,
            },
            None => where_it_started(working, Standing::WhereItStarted),
        },
        Packaging::Installed => match configuration {
            Some(configuration) => SettingsFolder {
                path: configuration.join(ACTER).join(SETTINGS),
                standing: Standing::Installed,
            },
            None => where_it_started(working, Standing::WhereItStarted),
        },
    }
}

/// The settings folder in the directory Acter was started from, which is both the
/// development answer and the last resort.
///
/// A machine that will not say where that is gets the relative name, which the operating
/// system resolves against the same directory — the behaviour that shipped before there was
/// a settings folder at all.
fn where_it_started(working: Option<&Path>, standing: Standing) -> SettingsFolder {
    SettingsFolder {
        path: working.map_or_else(|| PathBuf::from(SETTINGS), |at| at.join(SETTINGS)),
        standing,
    }
}

/// Where a portable copy keeps its settings, given the directory its program runs from.
///
/// **Beside the `.app` bundle on macOS rather than inside it** (decision 3). A file written
/// inside a bundle breaks its signature the moment M4 signs it, so a portable Mac copy
/// writes next to `Acter.app`, three levels above the executable in
/// `Acter.app/Contents/MacOS`. A macOS build that is *not* in a bundle writes beside its own
/// executable like every other platform, because there is no bundle to be outside of.
fn portable_settings(os: &str, program: &Path) -> PathBuf {
    match os {
        "macos" => beside_the_bundle(program).unwrap_or(program).join(SETTINGS),
        _ => program.join(SETTINGS),
    }
}

/// The directory holding the `.app` this executable is inside, or `None` when it is not
/// inside one.
fn beside_the_bundle(program: &Path) -> Option<&Path> {
    if program.file_name() != Some(OsStr::new("MacOS")) {
        return None;
    }
    let contents = program.parent()?;
    if contents.file_name() != Some(OsStr::new("Contents")) {
        return None;
    }
    let bundle = contents.parent()?;
    if !bundle
        .extension()
        .is_some_and(|extension| extension == "app")
    {
        return None;
    }
    bundle.parent()
}

/// What this build is, from what the build script stamped (decision 3).
///
/// **`option_env!` rather than `env!`**, because a build in a directory that is not a
/// repository stamps nothing at all and that is a state this product supports: a source
/// tarball with no git says what Cargo says.
fn stamped() -> Version {
    version(
        option_env!("VERGEN_GIT_DESCRIBE"),
        option_env!("VERGEN_GIT_SHA"),
        env!("CARGO_PKG_VERSION"),
    )
}

/// The rule turning what git said into the two things a person meets (decision 3).
///
/// - a describe string shaped `<platform>-vx.y.z`, with all three numbers numeric, is a
///   release, and the version is `x.y.z`;
/// - anything else with a commit behind it is `development-<short commit>`;
/// - nothing at all is `CARGO_PKG_VERSION`, which is what a source tarball with no git has.
///
/// **A tag that does not match the shape is not a release.** It falls to the development
/// case rather than being read out as a version, because a malformed tag claiming to be
/// `1.0` is worse than one saying it is a development build. It does not check that the
/// platform in the tag is the platform being built: which tag builds which artifact is the
/// release workflow's business (decision 21), not the binary's.
///
/// **A function rather than something the build script prints**, because these strings are
/// read aloud and a build script's output cannot be unit tested.
fn version(describe: Option<&str>, commit: Option<&str>, cargo: &str) -> Version {
    if let Some(number) = released(describe) {
        return Version::released(&number);
    }
    match commit.map(str::trim).filter(|commit| !commit.is_empty()) {
        Some(commit) => Version::development(commit),
        None => Version::from_cargo(cargo),
    }
}

/// The numeric triple in a release tag, or `None` for anything that is not one.
fn released(describe: Option<&str>) -> Option<String> {
    let (platform, number) = describe?.trim().rsplit_once("-v")?;
    if platform.is_empty() {
        return None;
    }
    let parts: Vec<&str> = number.split('.').collect();
    let [major, minor, patch] = parts[..] else {
        return None;
    };
    // Numeric and nothing else: `git describe` appends `-2-gf49246c` on a commit after the
    // tag, and that is a development build rather than a release of `0-2-gf49246c`.
    [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| number.to_owned())
}

/// The saved connection this launch asked for, from `acter --connect <name>` (spec 26,
/// decision 20).
///
/// **Parsed, never printed.** A windowed binary has no console, so there is nowhere to put
/// a usage message: an argument this does not recognise is ignored, and a name nothing is
/// saved under is answered in the window rather than on a stream nobody can hear.
///
/// `--connect=<name>` is accepted beside `--connect <name>`, because it is the other
/// spelling a person types and the alternative is a window that opens unconnected and never
/// says why.
///
/// Pure over the arguments it is given, so both spellings and every way of getting it wrong
/// are tested without a launch.
fn requested_connection<A: IntoIterator<Item = OsString>>(arguments: A) -> Option<String> {
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let named = if argument == CONNECT {
            arguments.next()
        } else {
            argument
                .to_str()
                .and_then(|argument| argument.strip_prefix(CONNECT))
                .and_then(|rest| rest.strip_prefix('='))
                .map(OsString::from)
        };
        if let Some(named) = named {
            return named
                .into_string()
                .ok()
                // A trailing space is invisible to the person who typed it, and `cmd.exe`
                // hands one over of its own accord (spec A9).
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
        }
    }
    None
}

/// The user's own `known_hosts`, which Acter reads and never writes (spec B9, decision 5) —
/// and `None` on a machine with no home directory to look in.
fn users_known_hosts() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".ssh").join("known_hosts"))
}

/// One reason, ended as a sentence.
///
/// **The reasons composed here come from the world, and the world does not punctuate.** A
/// transcript that is missing ends `(os error 2)`, and a pseudoconsole that will not open
/// ends with whatever the library said — so a reader runs straight on from the end of the
/// explanation into whatever it says next, with no pause where the thought finished. Every
/// user-facing string in this product is read aloud, which makes this a domain requirement
/// rather than typography (CLAUDE.md).
fn ended(reason: String) -> String {
    let trimmed = reason.trim_end();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.")
    }
}

/// Resolves a scripted profile's name to a far end and a delivery strategy.
///
/// **A name resolves to a composition, not to a file** (spec B6, decision 12, as B3.6
/// left it). Two of the three simulated shells worth naming stopped being transcripts:
/// "never emits markers" is a decorator over any far end, and "splits a marker across two
/// reads" is a property of the pipe carrying any transcript at all. So the table below is
/// the product of a shell and a chunking rather than a list of files somebody wrote, and
/// a path still names a transcript for everything that genuinely is one — a forged
/// marker, for instance, which is something a *program* does.
///
/// Reading the filesystem is the container's privilege: this is the composition root,
/// where the world is allowed in. **A name that resolves to nothing is a sentence rather
/// than a panic since B7**, for the reason every other failure to connect became one:
/// there is a window in front of the user now, and it can say what was wrong with what they
/// chose.
#[cfg(debug_assertions)]
fn far_end(name: &str) -> Result<(Box<dyn FakeShell>, Chunking), String> {
    Ok(match name {
        "builtin" => (Box::new(TranscriptShell::builtin()), Chunking::Whole),
        "builtin-by-byte" => (Box::new(TranscriptShell::builtin()), Chunking::Bytes(1)),
        "unmarked" => (
            Box::new(Unmarked::new(TranscriptShell::builtin())),
            Chunking::Whole,
        ),
        "unmarked-by-byte" => (
            Box::new(Unmarked::new(TranscriptShell::builtin())),
            Chunking::Bytes(1),
        ),
        path => (
            Box::new(TranscriptShell::new(transcript(path)?)),
            Chunking::Whole,
        ),
    })
}

#[cfg(debug_assertions)]
fn transcript(path: &str) -> Result<SessionTranscript, String> {
    SessionTranscript::load(path).map_err(|why| {
        format!(
            "Acter could not start the scripted session {path}. It is not one of the \
             built-in names (builtin, builtin-by-byte, unmarked, unmarked-by-byte), and it \
             could not be loaded as a transcript file: {why}"
        )
    })
}

#[cfg(test)]
mod tests {
    use acter_core::ShellMarkers;

    use super::*;

    /// **What the window offers is the platform's list, wired through the real service** — a
    /// composition-root assertion rather than a policy one, because `offered` being right and
    /// `state()` actually passing it are two different claims and only the second one is what
    /// a user gets (M1).
    ///
    /// **Asked for both platforms from whichever one this runs on**, which is the whole point
    /// of the list being a value: before M1 this could not have been written at all.
    #[test]
    fn the_window_offers_the_kinds_this_operating_system_has() {
        let windows = state_on("windows").connectable();
        let macos = state_on("macos").connectable();

        assert!(
            windows
                .iter()
                .any(|row| row.label.starts_with("Command Prompt")),
            "a Windows window offers the shell Windows always has"
        );
        assert!(
            !macos
                .iter()
                .any(|row| row.label.starts_with("Command Prompt") || row.label.starts_with("WSL")),
            "and a Mac is not offered Windows shells, not even as unavailable"
        );
        for (os, listed) in [("windows", &windows), ("macos", &macos)] {
            assert!(
                listed.iter().any(|row| row.label == "SSH"),
                "{os} can reach a far end that is not on this machine"
            );
        }
    }

    /// **And the build's own answer is one of them**, which is the line `state()` differs from
    /// [`state_on`] by. A composition root that read the platform and then passed a different
    /// list would pass every test above and ship the wrong window.
    #[test]
    fn the_build_offers_what_its_own_operating_system_offers() {
        let wired: Vec<_> = state(&settings())
            .connectable()
            .into_iter()
            .map(|row| row.label)
            .collect();
        let named: Vec<_> = state_on(consts::OS)
            .connectable()
            .into_iter()
            .map(|row| row.label)
            .collect();

        assert_eq!(wired, named, "{} gets its own list", consts::OS);
    }

    /// **Where Acter keeps its settings, for every packaging on both platforms**
    /// (spec 26, decision 4).
    ///
    /// **It is a test because the answer used to be silently wrong** (M1), and because the
    /// packaging arrives as a *value*: a `#[cfg]` on the rule would compile half of it out
    /// of every test run, so a Windows machine could never assert the macOS answers and a
    /// Mac could never assert Windows'. Every case below runs on both.
    mod where_acter_keeps_its_settings {
        use super::*;

        /// A machine that answers every question, so each test below can vary the one it
        /// is about.
        fn windows() -> PathBuf {
            PathBuf::from(r"C:\Users\someone\AppData\Roaming")
        }

        fn mac() -> PathBuf {
            PathBuf::from("/Users/someone/Library/Application Support")
        }

        fn program() -> PathBuf {
            PathBuf::from(r"D:\portable\acter")
        }

        fn working() -> PathBuf {
            PathBuf::from(r"C:\projects\acter")
        }

        fn folder(
            os: &str,
            packaging: Packaging,
            configuration: Option<PathBuf>,
            program: Option<PathBuf>,
        ) -> SettingsFolder {
            settings_folder(
                os,
                packaging,
                configuration.as_deref(),
                program.as_deref(),
                Some(&working()),
                None,
            )
        }

        /// **A developer needs no variable set to get a sane answer**, which is the whole
        /// reason this packaging exists.
        #[test]
        fn a_development_build_keeps_them_where_it_was_started() {
            for os in ["windows", "macos"] {
                let at = folder(os, Packaging::Development, Some(windows()), Some(program()));

                assert_eq!(at.path, working().join("settings"), "{os}");
                assert_eq!(at.standing, Standing::Development, "{os}");
            }
        }

        #[test]
        fn a_portable_copy_keeps_them_beside_the_program() {
            let at = folder(
                "windows",
                Packaging::Portable,
                Some(windows()),
                Some(program()),
            );

            assert_eq!(at.path, program().join("settings"));
            assert_eq!(at.standing, Standing::Portable);
        }

        /// **Beside the bundle rather than inside it** (decision 3): a file written inside
        /// an `.app` breaks its signature the moment M4 signs it.
        #[test]
        fn a_portable_mac_keeps_them_outside_the_bundle() {
            let inside = PathBuf::from("/Applications/Acter.app/Contents/MacOS");

            let at = folder("macos", Packaging::Portable, Some(mac()), Some(inside));

            assert_eq!(at.path, PathBuf::from("/Applications/settings"));
            assert_eq!(at.standing, Standing::Portable);
        }

        /// And a macOS build that is not in a bundle is beside its own executable like
        /// every other platform, because there is no bundle to be outside of.
        #[test]
        fn a_portable_mac_that_is_not_in_a_bundle_is_beside_its_own_program() {
            let loose = PathBuf::from("/Users/someone/acter");

            let at = folder(
                "macos",
                Packaging::Portable,
                Some(mac()),
                Some(loose.clone()),
            );

            assert_eq!(at.path, loose.join("settings"));
        }

        /// A directory that merely *looks* like part of a bundle is not one, so nothing is
        /// written a level up from somewhere it should not be.
        #[test]
        fn only_a_real_bundle_puts_the_settings_a_level_up() {
            for not_a_bundle in [
                "/Users/someone/MacOS",
                "/Users/someone/Contents/MacOS",
                "/Users/someone/Acter.zip/Contents/MacOS",
            ] {
                let at = folder(
                    "macos",
                    Packaging::Portable,
                    Some(mac()),
                    Some(PathBuf::from(not_a_bundle)),
                );

                assert_eq!(
                    at.path,
                    PathBuf::from(not_a_bundle).join("settings"),
                    "{not_a_bundle}"
                );
            }
        }

        /// **The installed case is one branch on both platforms**, which is what asking
        /// the operating system for the account's configuration directory buys: each
        /// answers its own base and both then join the same suffix.
        #[test]
        fn an_installed_copy_keeps_them_with_the_accounts_configuration() {
            let on_windows = folder(
                "windows",
                Packaging::Installed,
                Some(windows()),
                Some(program()),
            );
            assert_eq!(
                on_windows.path,
                windows().join("acter").join("settings"),
                "Windows keeps it under the roaming profile"
            );
            assert_eq!(on_windows.standing, Standing::Installed);

            let on_a_mac = folder("macos", Packaging::Installed, Some(mac()), Some(program()));
            assert_eq!(
                on_a_mac.path,
                mac().join("acter").join("settings"),
                "macOS keeps it where Finder and Time Machine expect it, not in a dotfile"
            );
            assert_eq!(on_a_mac.standing, Standing::Installed);
        }

        /// **The variable wins over the packaging**, whichever packaging it is: it is what
        /// points the suites and the NVDA fixture at a directory made for them, and the one
        /// way to keep settings somewhere the packaging did not choose.
        #[test]
        fn the_variable_wins_over_every_packaging() {
            let told = PathBuf::from(r"E:\fixtures\acter");

            for packaging in [
                Packaging::Development,
                Packaging::Portable,
                Packaging::Installed,
            ] {
                let at = settings_folder(
                    "windows",
                    packaging,
                    Some(&windows()),
                    Some(&program()),
                    Some(&working()),
                    Some(&told),
                );

                assert_eq!(at.path, told, "{packaging:?}");
                assert_eq!(at.standing, Standing::Directed, "{packaging:?}");
            }
        }

        /// A machine that reports no configuration directory at all — a service account, a
        /// stripped environment — gets the folder Acter was started from, and About says
        /// so rather than dressing it up as an installation.
        #[test]
        fn a_machine_that_says_nothing_about_itself_falls_back_and_says_which() {
            let installed = folder("windows", Packaging::Installed, None, Some(program()));
            assert_eq!(installed.path, working().join("settings"));
            assert_eq!(installed.standing, Standing::WhereItStarted);

            let portable = folder("windows", Packaging::Portable, Some(windows()), None);
            assert_eq!(portable.path, working().join("settings"));
            assert_eq!(portable.standing, Standing::WhereItStarted);
        }

        /// And one that will not even say where it was started from still gets a folder:
        /// the relative name, which the operating system resolves against that same
        /// directory. This is the behaviour that shipped before there was a settings folder.
        #[test]
        fn a_machine_that_will_not_say_where_it_is_still_gets_a_folder() {
            let at = settings_folder("windows", Packaging::Installed, None, None, None, None);

            assert_eq!(at.path, PathBuf::from("settings"));
            assert_eq!(at.standing, Standing::WhereItStarted);
        }

        /// **The build's own packaging matches its feature**, which is the line the tests
        /// above cannot reach: they all pass a packaging in, and a `packaging()` that
        /// answered the wrong one would ship a copy writing somewhere nobody expected.
        #[test]
        fn the_build_is_packaged_the_way_its_features_say() {
            if cfg!(feature = "portable") {
                assert_eq!(
                    packaging(),
                    Packaging::Portable,
                    "the feature wins over the debug default, so a portable build can be \
                     driven on a developer's machine"
                );
            } else if cfg!(debug_assertions) {
                assert_eq!(packaging(), Packaging::Development);
            } else {
                assert_eq!(
                    packaging(),
                    Packaging::Installed,
                    "a release without the feature is what the installer ships"
                );
            }
        }
    }

    /// **What this build is, as a bug report carries it and as About says it** (spec 26,
    /// decision 3). The rule is a pure function precisely because these strings are read
    /// aloud and a build script's output cannot be unit tested.
    mod what_this_build_is {
        use super::*;

        const CARGO: &str = "0.1.0";

        /// A tag shaped `<platform>-vx.y.z` is a release, and **the version is the numeric
        /// triple and nothing else**: a listener hears "Version 1.0.0" on either platform,
        /// because the platform is a fact about which file they downloaded.
        #[test]
        fn a_platform_tag_is_a_release_and_the_platform_is_not_in_the_version() {
            for tag in ["windows-v1.0.0", "macos-v1.0.0"] {
                let version = version(Some(tag), Some("521c956"), CARGO);

                assert_eq!(version.identifier, "1.0.0", "{tag}");
                assert_eq!(version.said, "Version 1.0.0.", "{tag}");
            }
        }

        /// Anything else with a commit behind it is a development build, and the commit is
        /// what a bug report carries.
        #[test]
        fn a_commit_with_no_release_tag_is_a_development_build() {
            let version = version(None, Some("521c956"), CARGO);

            assert_eq!(version.identifier, "development-521c956");
            assert_eq!(version.said, "Development build, commit 521c956.");
        }

        /// **A malformed tag falls to the development case rather than being read out**,
        /// because a tag claiming to be `1.0` said aloud as a version is worse than one
        /// saying it is a development build.
        #[test]
        fn a_tag_that_is_not_shaped_like_a_release_is_not_read_out_as_one() {
            for tag in [
                "v1.0.0",
                "windows-v1.0",
                "windows-v1.0.0.0",
                "windows-v1.0.x",
                "windows-1.0.0",
                "windows-v",
                "windows-v1.0.0-2-gf49246c",
                "windows-v..",
            ] {
                let version = version(Some(tag), Some("521c956"), CARGO);

                assert_eq!(
                    version.identifier, "development-521c956",
                    "{tag} is not a release"
                );
            }
        }

        /// Nothing at all is what a source tarball with no git has, and it says what Cargo
        /// says.
        #[test]
        fn a_tree_with_no_git_says_what_cargo_says() {
            let version = version(None, None, CARGO);

            assert_eq!(version.identifier, CARGO);
            assert_eq!(version.said, "Version 0.1.0.");
        }

        /// A commit the build stamped as an empty string is no commit, which is what a
        /// release workflow overriding one variable and not the other would produce.
        #[test]
        fn a_commit_that_is_not_there_is_not_a_development_build() {
            assert_eq!(version(None, Some("   "), CARGO).identifier, CARGO);
        }

        /// Every sentence a listener hears is a whole one, and none of them tries to spell
        /// a commit out loud beyond naming it once.
        #[test]
        fn every_version_sentence_is_one_a_reader_can_speak() {
            for version in [
                version(Some("windows-v1.0.0"), Some("521c956"), CARGO),
                version(None, Some("521c956"), CARGO),
                version(None, None, CARGO),
            ] {
                assert!(version.said.ends_with('.'), "{}", version.said);
                assert!(!version.said.contains("  "), "{}", version.said);
                assert!(!version.identifier.trim().is_empty());
            }
        }
    }

    /// **What a launch asked to connect to, in both spellings a person types** (spec 26,
    /// decision 20). Nothing here starts anything: the switch becomes a request the window
    /// carries out, so a saved SSH connection can ask its questions where somebody can hear
    /// them.
    #[test]
    fn the_launch_switch_names_the_saved_connection_it_asked_for() {
        let asked = |arguments: &[&str]| {
            requested_connection(arguments.iter().map(|argument| OsString::from(*argument)))
        };

        assert_eq!(
            asked(&["--connect", "work laptop"]).as_deref(),
            Some("work laptop")
        );
        assert_eq!(
            asked(&["--connect=work laptop"]).as_deref(),
            Some("work laptop"),
            "the other spelling of the same switch"
        );
        assert_eq!(
            asked(&["--connect", " work laptop "]).as_deref(),
            Some("work laptop"),
            "a name is trimmed, because a trailing space is invisible to the person who typed it"
        );
    }

    /// An ordinary launch, and every way of getting the switch wrong: all of them open the
    /// window unconnected, because a windowed binary has no console to print a usage
    /// message to and an argument nobody recognises is not worth refusing to start over.
    #[test]
    fn a_launch_that_names_nothing_asks_for_nothing() {
        let asked = |arguments: &[&str]| {
            requested_connection(arguments.iter().map(|argument| OsString::from(*argument)))
        };

        assert_eq!(asked(&[]), None, "an ordinary launch");
        assert_eq!(
            asked(&["--connect"]),
            None,
            "the switch with no name after it"
        );
        assert_eq!(
            asked(&["--connect", "   "]),
            None,
            "a name that is only spaces"
        );
        assert_eq!(
            asked(&["--verbose", "work"]),
            None,
            "an argument nobody reads"
        );
        assert_eq!(
            asked(&["--connected", "work"]),
            None,
            "a switch that is not this one"
        );
    }

    /// **The release gate, asserted rather than assumed.** What a build offers and what it
    /// can construct are the same decision, so this pins both halves against the build it
    /// is running in: a debug build lists the four scripted sessions, and a release build
    /// lists none at all.
    #[test]
    fn only_a_debug_build_offers_a_scripted_session() {
        let offered = scripted_profiles();

        if cfg!(debug_assertions) {
            assert_eq!(
                offered,
                ["builtin", "builtin-by-byte", "unmarked", "unmarked-by-byte"],
                "a development build offers every scripted far end"
            );
        } else {
            assert!(
                offered.is_empty(),
                "a release build never names a scripted session"
            );
        }
    }

    /// **The connection sentences, asserted as whole clauses because that is what a listener
    /// hears** (spec B9.5, decision 13): the announcement is this string appended to the row's
    /// own label, so "Ubuntu 24.04" plus what is below is the utterance.
    #[test]
    fn a_session_that_was_set_up_is_named_and_nothing_more_is_said_about_it() {
        let note = far_end_note(Some("bash"), SetUpOutcome::Fully);

        assert_eq!(note.said.as_deref(), Some("bash"));
        assert!(
            !note.limit_explained,
            "nothing has been said about a limit, so the grace period may still speak"
        );
    }

    /// **The only case where the grace period will never speak**, because markers do arrive —
    /// so this clause is the one chance to say what a listener will not get (spec B9.5,
    /// decision 8).
    ///
    /// **The name here is a shape rather than a shell.** It was `sh` until roadmap 23.15
    /// measured `sh` reporting exit codes after all, and nothing that ships reaches this
    /// outcome today; what it is kept for is the next shell somebody measures whose prompt is
    /// all it has, and the sentence such a listener would get.
    #[test]
    fn a_session_set_up_only_as_far_as_its_prompt_says_what_it_cannot_do() {
        let note = far_end_note(Some("ksh"), SetUpOutcome::Partly);

        assert_eq!(
            note.said.as_deref(),
            Some(
                "ksh. Acter can tell when a command has finished here, but not whether it worked."
            )
        );
        assert!(note.limit_explained);
    }

    /// **The reason B5.5 currently only implies**, said out loud: the shell is named, and the
    /// listener is told that Acter has nothing measured for it rather than being left to infer
    /// it from five seconds of silence.
    #[test]
    fn a_shell_nobody_measured_is_named_and_said_to_be_one() {
        let note = far_end_note(Some("fish"), SetUpOutcome::NothingWritten);

        assert_eq!(
            note.said.as_deref(),
            Some("fish, which Acter cannot set up yet.")
        );
        assert!(note.limit_explained);
    }

    /// **What a listener hears when they said no**, whether by unticking the Connect dialog's
    /// checkbox or by cancelling the dialog: the name, and A13's own sentence about what this
    /// session will and will not tell them.
    #[test]
    fn a_session_that_was_not_set_up_says_what_it_will_and_will_not_tell_them() {
        let note = far_end_note(Some("bash"), SetUpOutcome::Refused);

        assert_eq!(
            note.said.as_deref(),
            Some("bash. You will hear what commands print here, but not whether they worked.")
        );
        assert!(note.limit_explained);
    }

    /// **Nothing answered: no clause at all.** The name is not invented, and the sentence that
    /// covers this case is A13's, said when the grace period expires — so this one must leave
    /// room for it rather than saying the same thing in another register, which is what
    /// roadmap 13.8 objected to.
    #[test]
    fn a_far_end_that_answered_nothing_invents_no_name_and_leaves_the_sentence_to_a13() {
        for outcome in [
            SetUpOutcome::Fully,
            SetUpOutcome::Partly,
            SetUpOutcome::NothingWritten,
            SetUpOutcome::Refused,
        ] {
            let note = far_end_note(None, outcome);

            assert_eq!(note.said, None, "{outcome:?} invents no name");
            assert!(
                !note.limit_explained,
                "{outcome:?} leaves the grace period free to say it"
            );
        }
    }

    /// **The phrase A13 removed is in none of them, which is the whole of roadmap 13.8.**
    /// "Shell integration" is project vocabulary a user does not have, and this is the *first*
    /// thing a listener hears on connecting.
    #[test]
    fn no_connection_sentence_uses_the_vocabulary_a13_removed() {
        for outcome in [
            SetUpOutcome::Fully,
            SetUpOutcome::Partly,
            SetUpOutcome::NothingWritten,
            SetUpOutcome::Refused,
        ] {
            let Some(said) = far_end_note(Some("bash"), outcome).said else {
                continue;
            };

            for forbidden in ["shell integration", "integrated", "marker", "exit code"] {
                assert!(
                    !said.to_lowercase().contains(forbidden),
                    "{outcome:?} says {forbidden:?} to a listener: {said}"
                );
            }
        }
    }

    /// Every clause a listener hears ends as a sentence does, so a reader pauses where the
    /// thought finished — except the bare name, which is a noun the label runs into.
    #[test]
    fn every_clause_that_is_a_sentence_ends_like_one() {
        for outcome in [
            SetUpOutcome::Partly,
            SetUpOutcome::NothingWritten,
            SetUpOutcome::Refused,
        ] {
            let said = far_end_note(Some("bash"), outcome)
                .said
                .expect("this outcome always says something");

            assert!(said.ends_with('.'), "{outcome:?} does not end: {said}");
        }
    }

    /// A far end with nothing to add adds nothing, which is every local shell the row the
    /// user chose already described — the case `only` builds by construction.
    #[test]
    fn a_far_end_with_nothing_to_say_says_nothing() {
        assert_eq!(far_end_note(None, SetUpOutcome::NothingWritten).said, None);
    }

    /// **What is heard during the five to six seconds a cold distribution takes to boot.**
    /// A whole sentence, because it is read aloud exactly as it arrives, and it names the
    /// distribution the user chose rather than describing what Acter is doing to it.
    #[test]
    fn a_distribution_that_is_starting_says_so_before_the_wait() {
        assert_eq!(starting(Some("Ubuntu 24.04")), "Starting Ubuntu 24.04.");
        assert_eq!(
            starting(None),
            "Starting Linux.",
            "a launch that named no distribution invents no name for WSL's default"
        );
        for said in [starting(Some("Debian")), starting(None)] {
            assert!(
                said.ends_with('.'),
                "a spoken sentence ends in a full stop, so a reader pauses: {said}"
            );
        }
    }

    /// The launch path, without a launch: which profile each environment resolves to, and
    /// that neither variable resolves to nothing at all — which is the window B7 opens.
    ///
    /// The variables are read rather than set, because a test that sets a process-wide
    /// environment variable is a test that changes what every other test in the binary sees.
    #[test]
    fn a_shell_and_a_transcript_resolve_to_the_two_profiles_that_can_carry_them() {
        assert_eq!(
            ProfileId::Program {
                program: "powershell.exe".to_owned()
            }
            .label(),
            "powershell",
            "a named program is labelled by its name, which is what the window shows"
        );
        assert_eq!(
            ProfileId::Scripted {
                name: "builtin".to_owned()
            }
            .label(),
            "Scripted: builtin"
        );
    }

    /// A scripted name nobody wrote a transcript for is a sentence a listener can hear,
    /// rather than the panic it was until B7.
    #[cfg(debug_assertions)]
    #[test]
    fn a_scripted_name_that_is_not_a_transcript_says_so_in_a_sentence() {
        let Err(why) = far_end("no-such-transcript.json") else {
            panic!("there is no such transcript to load");
        };

        assert!(why.starts_with("Acter could not start"), "{why}");
        assert!(
            why.contains("builtin"),
            "and names what it would have accepted: {why}"
        );
    }

    /// **The world does not punctuate, and this is what a listener meets when it does not.**
    /// A missing transcript ends `(os error 2)`; without this the sentence about it ran
    /// straight on into whatever the reader said next, with no pause where the thought
    /// finished.
    #[test]
    fn a_reason_the_world_wrote_is_ended_before_it_is_spoken() {
        assert_eq!(
            ended("The system cannot find the file specified. (os error 2)".to_owned()),
            "The system cannot find the file specified. (os error 2)."
        );
        assert_eq!(
            ended("Access is denied.".to_owned()),
            "Access is denied.",
            "a reason that already ends is left alone"
        );
        assert_eq!(
            ended("Access is denied.  ".to_owned()),
            "Access is denied.",
            "including one that ends in trailing space"
        );
    }

    /// Whether a session is set up, and what it is told when it is not (spec B9.5,
    /// decisions 9, 10 and 13).
    mod the_checkbox_authorises_and_the_dialog_discloses {
        use std::sync::Mutex;

        use acter_core::{
            HostKeyAnswer, HostKeyQuestion, PasswordQuestion, ProgramAnswer, ProgramQuestion,
            Secret, SessionSetup, SshQuestions,
        };

        use super::*;

        /// A record of what has been explained, held in memory: the file adapter's behaviour
        /// is its own to test, and this is about what the factory does with the answer.
        #[derive(Default)]
        struct Remembered(Mutex<Vec<String>>);

        impl Explained for Remembered {
            fn already(&self, shell: &str) -> bool {
                self.0.lock().unwrap().iter().any(|at| at == shell)
            }

            fn remember(&self, shell: &str) {
                self.0.lock().unwrap().push(shell.to_owned());
            }
        }

        /// Somebody who answers the setup question the way a test says, and refuses
        /// everything else — so nothing here can pass by answering the wrong question.
        struct Answering {
            answer: SetupAnswer,
            asked: Mutex<Vec<SetupQuestion>>,
        }

        impl Answering {
            fn with(answer: SetupAnswer) -> Arc<Self> {
                Arc::new(Self {
                    answer,
                    asked: Mutex::new(Vec::new()),
                })
            }
        }

        impl SshQuestions for Answering {
            fn host_key(&self, _question: HostKeyQuestion) -> HostKeyAnswer {
                HostKeyAnswer::Refuse
            }
            fn password(&self, _question: PasswordQuestion) -> Option<Secret> {
                None
            }
            fn tell(&self, _sentence: &str) {}
        }

        impl ConnectQuestions for Answering {
            fn unverified(&self, _question: ProgramQuestion) -> ProgramAnswer {
                ProgramAnswer::DoNotStart
            }

            fn set_up_session(&self, question: SetupQuestion) -> SetupAnswer {
                self.asked.lock().unwrap().push(question);
                self.answer
            }
        }

        fn factory(explained: Arc<dyn Explained>) -> Shells {
            Shells {
                clock: Arc::new(SystemClock::new()),
                machine: machine(),
                explained,
                // Nothing here reaches a server, so nothing here writes one down.
                known_hosts: Arc::new(acter_core::RememberedHostKeys::default()),
            }
        }

        fn facts(shell: &str) -> ShellFacts {
            let setup = acter_shells::setup_for(Some(shell));
            ShellFacts {
                markers: setup
                    .as_ref()
                    .map(|setup| setup.markers)
                    .unwrap_or(ShellMarkers::Full),
                eof: None,
                setup,
                discards_line: None,
            }
        }

        /// The ordinary case: the box is ticked, the person has not seen this shell's command
        /// before, they are shown it, and they say yes.
        #[test]
        fn a_shell_that_was_agreed_to_is_set_up_and_named() {
            let asker = Answering::with(SetupAnswer::SetUp { remember: false });
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, outcome) = factory(Arc::new(Remembered::default())).agreed(
                facts("bash"),
                Some("bash"),
                SetUp::Yes,
                &questions,
            );

            assert_eq!(outcome, SetUpOutcome::Fully);
            assert!(facts.setup.is_some(), "the line goes to the session");
            let asked = asker.asked.lock().unwrap();
            assert_eq!(asked.len(), 1, "asked once");
            assert_eq!(asked[0].shell, "bash", "and it names what it detected");
        }

        /// **Unticking the box skips both the dialog and the setup** (spec B9.5, decision 9),
        /// which is what makes refusing reachable without the dialog ever appearing.
        #[test]
        fn an_unticked_box_asks_nothing_and_runs_nothing() {
            let asker = Answering::with(SetupAnswer::SetUp { remember: false });
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, outcome) = factory(Arc::new(Remembered::default())).agreed(
                facts("bash"),
                Some("bash"),
                SetUp::No,
                &questions,
            );

            assert_eq!(outcome, SetUpOutcome::Refused);
            assert_eq!(facts.setup, None, "nothing is sent into the session");
            assert!(
                asker.asked.lock().unwrap().is_empty(),
                "and nobody is asked about a setup that is not going to happen"
            );
        }

        /// **Cancelling refuses this session only, and the session still runs** — and the
        /// marker claim goes back to the optimistic default, so the grace period is what tells
        /// the listener the truth about it.
        #[test]
        fn cancelling_leaves_the_session_running_and_unmodified() {
            let asker = Answering::with(SetupAnswer::Skip);
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, outcome) = factory(Arc::new(Remembered::default())).agreed(
                facts("sh"),
                Some("sh"),
                SetUp::Yes,
                &questions,
            );

            assert_eq!(outcome, SetUpOutcome::Refused);
            assert_eq!(facts.setup, None);
            assert_eq!(
                facts.markers,
                ShellMarkers::Full,
                "a claim the grace period can contradict, rather than one nothing will"
            );
        }

        /// **Asked once per shell per person, and per shell is the decision** (spec B9.5,
        /// decision 10): somebody who has read and accepted what Acter runs in bash has not
        /// thereby accepted what it runs in another shell, because it is a different command.
        #[test]
        fn do_not_show_this_again_is_remembered_for_that_shell_and_for_no_other() {
            let explained = Arc::new(Remembered::default());
            let asker = Answering::with(SetupAnswer::SetUp { remember: true });
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;
            let shells = factory(Arc::clone(&explained) as Arc<dyn Explained>);

            shells.agreed(facts("bash"), Some("bash"), SetUp::Yes, &questions);
            shells.agreed(facts("bash"), Some("bash"), SetUp::Yes, &questions);
            let (facts, outcome) = shells.agreed(facts("sh"), Some("sh"), SetUp::Yes, &questions);

            let asked = asker.asked.lock().unwrap();
            assert_eq!(
                asked.len(),
                2,
                "bash asked once and never again; sh asked for itself"
            );
            assert_eq!(asked[1].shell, "sh");
            assert_eq!(
                outcome,
                SetUpOutcome::Fully,
                "and sh is set up as far as its own line reaches, which since roadmap 23.15                  is a verdict as well as a heading"
            );
            assert!(facts.setup.is_some());
        }

        /// **A shell with nothing written for it produces no dialog**, because there is no
        /// command to show. The connection sentence says so instead and the session goes on.
        #[test]
        fn a_shell_nobody_measured_is_named_with_no_dialog_and_no_wait() {
            let asker = Answering::with(SetupAnswer::Skip);
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, outcome) = factory(Arc::new(Remembered::default())).agreed(
                facts("fish"),
                Some("fish"),
                SetUp::Yes,
                &questions,
            );

            assert_eq!(outcome, SetUpOutcome::NothingWritten);
            assert_eq!(facts.setup, None);
            assert!(asker.asked.lock().unwrap().is_empty(), "nothing to show");
            assert_eq!(
                far_end_note(Some("fish"), outcome).said.as_deref(),
                Some("fish, which Acter cannot set up yet.")
            );
        }

        /// A far end that answered nothing is asked about nothing, whatever the checkbox said:
        /// there is no shell to name in the dialog and nothing measured to run.
        #[test]
        fn a_far_end_that_answered_nothing_is_asked_about_nothing() {
            let asker = Answering::with(SetupAnswer::SetUp { remember: false });
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, outcome) = factory(Arc::new(Remembered::default())).agreed(
                facts("nothing-answered"),
                None,
                SetUp::Yes,
                &questions,
            );

            assert_eq!(outcome, SetUpOutcome::NothingWritten);
            assert_eq!(facts.setup, None);
            assert!(asker.asked.lock().unwrap().is_empty());
        }

        /// **The command is shown before it runs, verbatim** (spec B9.5, decision 3): what the
        /// dialog puts in its read-only field is the same string the session submits.
        #[test]
        fn the_question_carries_the_command_that_would_run() {
            let asker = Answering::with(SetupAnswer::SetUp { remember: false });
            let questions = Arc::clone(&asker) as Arc<dyn ConnectQuestions>;

            let (facts, _) = factory(Arc::new(Remembered::default())).agreed(
                facts("bash"),
                Some("bash"),
                SetUp::Yes,
                &questions,
            );

            let asked = asker.asked.lock().unwrap();
            assert_eq!(
                asked[0].setup,
                facts.setup.clone().expect("bash is set up"),
                "the disclosure and the submission are one string"
            );
            assert_eq!(
                asked[0].setup,
                SessionSetup {
                    line: acter_shells::setup_for(Some("bash")).unwrap().line,
                    markers: ShellMarkers::Full,
                }
            );
        }
    }
}
