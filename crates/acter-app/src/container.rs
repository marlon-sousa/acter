//! Controller (orchestrator): the container (composition root) — the only place
//! where concrete implementations are constructed and bound to their ports, where the
//! environment is read, and where the Tauri runtime is started.

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

const TRANSCRIPT_ENV: &str = "ACTER_TRANSCRIPT";

const SHELL_ENV: &str = "ACTER_SHELL";

const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

const SCRIPTED: &str = "scripted";

const WSL_CLIENT: &str = "wsl.exe";

const SETTINGS_DIR: &str = "ACTER_SETTINGS_DIR";

const SETTINGS: &str = "settings";

const CONNECT: &str = "--connect";

const ACTER: &str = "acter";

pub(crate) struct AppState {
    pub(crate) session: Arc<dyn SessionApi>,
    pub(crate) connect: Arc<dyn ConnectApi>,
    pub(crate) connecting: Arc<Connecting>,
    pub(crate) settings: Arc<Settings>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsFolder {
    pub(crate) path: PathBuf,
    pub(crate) standing: Standing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    Development,
    Portable,
    Installed,
    Directed,
    WhereItStarted,
}

impl Standing {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packaging {
    Development,
    Portable,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    pub(crate) identifier: String,
    pub(crate) said: String,
}

impl Version {
    pub(crate) fn released(number: &str) -> Self {
        Self {
            identifier: number.to_owned(),
            said: format!("Version {number}."),
        }
    }

    pub(crate) fn development(commit: &str) -> Self {
        Self {
            identifier: format!("development-{commit}"),
            said: format!("Development build, commit {commit}."),
        }
    }

    fn from_cargo(number: &str) -> Self {
        Self::released(number)
    }
}

pub fn run() {
    // Sessions spawn tasks and arm timers, so the state must be built inside Tauri's runtime.
    let runtime = tauri::async_runtime::handle();
    let state = {
        let _entered = runtime.inner().enter();
        connected_state()
    };

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

    #[cfg(debug_assertions)]
    let builder = {
        use tauri_plugin_wdio_webdriver::init;
        builder.plugin(init())
    };

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

pub(crate) fn connected_state() -> AppState {
    // Resolved once and shared: a second lookup could name a folder the files are not in.
    let settings = settings();
    let service = Arc::new(state(&settings));
    if let Some(profile) = launch_profile() {
        // The error is dropped: `connected()` then answers `None` and the window opens unconnected.
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

pub(crate) fn settings() -> Arc<Settings> {
    Arc::new(Settings::open(settings_directory(), stamped()))
}

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

#[cfg(windows)]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(WindowsTrust::new())
}

#[cfg(target_os = "macos")]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(AppleTrust::new())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn signatures() -> Arc<dyn Signatures> {
    Arc::new(acter_core::Unchecked)
}

#[cfg(windows)]
fn machine() -> Arc<dyn ThisComputer> {
    Arc::new(WindowsMachine::new())
}

#[cfg(not(windows))]
fn machine() -> Arc<dyn ThisComputer> {
    Arc::new(UnixMachine::new())
}

/// `None` means the window starts unconnected.
fn launch_profile() -> Option<ProfileId> {
    if let Ok(program) = env::var(SHELL_ENV) {
        return Some(ProfileId::Program { program });
    }
    env::var(TRANSCRIPT_ENV)
        .ok()
        .map(|name| ProfileId::Scripted { name })
}

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

struct Shells {
    clock: Arc<dyn Clock>,
    machine: Arc<dyn ThisComputer>,
    explained: Arc<dyn Explained>,
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

    fn session(
        &self,
        transport: Box<dyn Transport>,
        shell: &dyn ShellAdapter,
    ) -> Arc<dyn SessionApi> {
        self.session_with(transport, ShellFacts::of(shell))
    }

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

    fn agreed(
        &self,
        facts: ShellFacts,
        shell: Option<&str>,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> (ShellFacts, SetUpOutcome) {
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
        let outcome = if facts.markers.reports_exit_code() {
            SetUpOutcome::Fully
        } else {
            SetUpOutcome::Partly
        };
        (facts, outcome)
    }

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

    fn wsl(
        &self,
        client: &str,
        distribution: Option<&str>,
        set_up: SetUp,
        questions: &Arc<dyn ConnectQuestions>,
    ) -> Result<Started, String> {
        questions.tell(&starting(distribution));

        let machine = Arc::clone(&self.machine);
        let named = distribution.map(ToOwned::to_owned);
        let asking = std::thread::spawn(move || machine.login_shell(named.as_deref()));

        let adapter = match distribution {
            Some(name) => Wsl::in_distribution(client, name, None),
            None => Wsl::new(client, None),
        };
        let pty = self.pty(&adapter.launch())?;
        // A probe whose thread died answered nothing: the session starts and nothing is claimed.
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

    /// The facts handed to the domain must come from the adapter that did the injecting, or the
    /// session receives markers it cannot use and speaks nothing.
    fn real(&self, shell: Box<dyn ShellAdapter>) -> Result<Arc<dyn SessionApi>, String> {
        let pty = self.pty(&shell.launch())?;
        Ok(self.session(Box::new(pty), shell.as_ref()))
    }

    fn pty(&self, launch: &ShellLaunch) -> Result<LocalPty, String> {
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        let environment: Vec<(&str, &str)> = launch
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
    }

    #[cfg(debug_assertions)]
    fn scripted(&self, name: &str) -> Result<Arc<dyn SessionApi>, String> {
        let (shell, chunking) = far_end(name)?;
        let transport = ScriptedTransport::with_shell(shell, chunking, Arc::clone(&self.clock));
        Ok(self.session(Box::new(transport), &Plain::new(SCRIPTED)))
    }
}

impl SessionFactory for Shells {
    /// `use_profile` runs on whichever thread Tauri answers an invoke on, so every branch enters
    /// the Tauri runtime before a session spawns its tasks.
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
    /// Local branches start `chosen.program`, the file the connect service verified; resolving
    /// the name again could land on a different file.
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
            ProfileId::Shell { kind } => {
                self.local(program.unwrap_or_else(|| kind.program()), set_up, questions)
            }
            ProfileId::Install { kind, .. } => {
                self.local(program.unwrap_or_else(|| kind.program()), set_up, questions)
            }
            ProfileId::Distribution { name } => {
                self.wsl(program.unwrap_or(WSL_CLIENT), Some(name), set_up, questions)
            }
            ProfileId::Program { program: named } => {
                self.local(program.unwrap_or_else(|| named.trim()), set_up, questions)
            }
            #[cfg(debug_assertions)]
            ProfileId::Scripted { name } => self.scripted(name).map(only),
            #[cfg(not(debug_assertions))]
            ProfileId::Scripted { name } => Err(format!(
                "The scripted session {name} is only available in a development build of \
                 Acter."
            )),
        }
    }
}

fn only(session: Arc<dyn SessionApi>) -> Started {
    Started {
        session,
        note: None,
        limit_explained: false,
    }
}

fn starting(distribution: Option<&str>) -> String {
    match distribution {
        Some(name) => format!("Starting {name}."),
        None => "Starting Linux.".to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetUpOutcome {
    Fully,
    Partly,
    NothingWritten,
    Refused,
}

struct Note {
    said: Option<String>,
    limit_explained: bool,
}

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

pub(crate) fn settings_directory() -> SettingsFolder {
    settings_folder(
        consts::OS,
        packaging(),
        dirs::config_dir().as_deref(),
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

/// `cfg!` rather than `#[cfg]`: every variant must still be constructed in every build, or
/// clippy fails the portable build on dead code.
fn packaging() -> Packaging {
    if cfg!(feature = "portable") {
        Packaging::Portable
    } else if cfg!(debug_assertions) {
        Packaging::Development
    } else {
        Packaging::Installed
    }
}

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

fn where_it_started(working: Option<&Path>, standing: Standing) -> SettingsFolder {
    SettingsFolder {
        path: working.map_or_else(|| PathBuf::from(SETTINGS), |at| at.join(SETTINGS)),
        standing,
    }
}

/// A portable macOS copy writes beside `Acter.app`, never inside it: a file written inside a
/// signed bundle breaks its signature.
fn portable_settings(os: &str, program: &Path) -> PathBuf {
    match os {
        "macos" => beside_the_bundle(program).unwrap_or(program).join(SETTINGS),
        _ => program.join(SETTINGS),
    }
}

/// `None` when the executable is not inside an `.app` bundle.
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

fn stamped() -> Version {
    version(
        option_env!("VERGEN_GIT_DESCRIBE"),
        option_env!("VERGEN_GIT_SHA"),
        env!("CARGO_PKG_VERSION"),
    )
}

fn version(describe: Option<&str>, commit: Option<&str>, cargo: &str) -> Version {
    if let Some(number) = released(describe) {
        return Version::released(&number);
    }
    match commit.map(str::trim).filter(|commit| !commit.is_empty()) {
        Some(commit) => Version::development(commit),
        None => Version::from_cargo(cargo),
    }
}

/// The version in a release tag, or `None` for anything that is not one.
fn released(describe: Option<&str>) -> Option<String> {
    // The *first* `-v`, because the platform never contains one and a suffix might.
    let (platform, number) = describe?.trim().split_once("-v")?;
    if platform.is_empty() || past_the_tag(number) {
        return None;
    }
    let (triple, suffix) = match number.split_once('-') {
        Some((triple, suffix)) => (triple, Some(suffix)),
        None => (number, None),
    };
    let parts: Vec<&str> = triple.split('.').collect();
    let [major, minor, patch] = parts[..] else {
        return None;
    };
    let numeric = [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    let named = suffix.is_none_or(|suffix| {
        !suffix.is_empty()
            && suffix.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
    });
    (numeric && named).then(|| number.to_owned())
}

/// Whether `number` ends in the `-<commits since>-g<commit>` tail `git describe` adds past a tag.
fn past_the_tag(number: &str) -> bool {
    let Some((before, commit)) = number.rsplit_once('-') else {
        return false;
    };
    let Some(hex) = commit.strip_prefix('g') else {
        return false;
    };
    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    before
        .rsplit_once('-')
        .is_some_and(|(_, since)| !since.is_empty() && since.bytes().all(|b| b.is_ascii_digit()))
}

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
                // See `named` in acter-core's protocol_commands.rs for the trailing space.
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty());
        }
    }
    None
}

/// `None` on a machine with no home directory to look in.
fn users_known_hosts() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".ssh").join("known_hosts"))
}

fn ended(reason: String) -> String {
    let trimmed = reason.trim_end();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.")
    }
}

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

    mod where_acter_keeps_its_settings {
        use super::*;

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

        #[test]
        fn a_portable_mac_keeps_them_outside_the_bundle() {
            let inside = PathBuf::from("/Applications/Acter.app/Contents/MacOS");

            let at = folder("macos", Packaging::Portable, Some(mac()), Some(inside));

            assert_eq!(at.path, PathBuf::from("/Applications/settings"));
            assert_eq!(at.standing, Standing::Portable);
        }

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

        #[test]
        fn a_machine_that_says_nothing_about_itself_falls_back_and_says_which() {
            let installed = folder("windows", Packaging::Installed, None, Some(program()));
            assert_eq!(installed.path, working().join("settings"));
            assert_eq!(installed.standing, Standing::WhereItStarted);

            let portable = folder("windows", Packaging::Portable, Some(windows()), None);
            assert_eq!(portable.path, working().join("settings"));
            assert_eq!(portable.standing, Standing::WhereItStarted);
        }

        #[test]
        fn a_machine_that_will_not_say_where_it_is_still_gets_a_folder() {
            let at = settings_folder("windows", Packaging::Installed, None, None, None, None);

            assert_eq!(at.path, PathBuf::from("settings"));
            assert_eq!(at.standing, Standing::WhereItStarted);
        }

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

    mod what_this_build_is {
        use super::*;

        const CARGO: &str = "0.1.0";

        #[test]
        fn a_platform_tag_is_a_release_and_the_platform_is_not_in_the_version() {
            for tag in ["windows-v1.0.0", "macos-v1.0.0"] {
                let version = version(Some(tag), Some("521c956"), CARGO);

                assert_eq!(version.identifier, "1.0.0", "{tag}");
                assert_eq!(version.said, "Version 1.0.0.", "{tag}");
            }
        }

        #[test]
        fn a_tag_with_a_suffix_after_the_three_numbers_is_a_release() {
            for (tag, number) in [
                ("windows-v1.0.0-alpha", "1.0.0-alpha"),
                ("windows-v1.0.0-beta", "1.0.0-beta"),
                ("macos-v2.3.4-rc.1", "2.3.4-rc.1"),
                ("windows-v1.0.0-beta-2", "1.0.0-beta-2"),
            ] {
                let version = version(Some(tag), Some("521c956"), CARGO);

                assert_eq!(version.identifier, number, "{tag}");
                assert_eq!(version.said, format!("Version {number}."), "{tag}");
            }
        }

        #[test]
        fn a_commit_past_a_pre_release_tag_is_not_that_pre_release() {
            for tag in [
                "windows-v1.0.0-2-gf49246c",
                "windows-v1.0.0-beta-2-gf49246c",
                "windows-v1.0.0-rc.1-14-g0803341",
            ] {
                let version = version(Some(tag), Some("521c956"), CARGO);

                assert_eq!(
                    version.identifier, "development-521c956",
                    "{tag} is past the tag"
                );
            }
        }

        #[test]
        fn a_suffix_that_is_not_one_is_not_a_release() {
            for tag in [
                "windows-v1.0.0-",
                "windows-v1.0.0-beta..1",
                "windows-v1.0.0-beta.",
                "windows-v1.0.0-be ta",
            ] {
                assert_eq!(
                    version(Some(tag), Some("521c956"), CARGO).identifier,
                    "development-521c956",
                    "{tag} is not a release"
                );
            }
        }

        #[test]
        fn a_commit_with_no_release_tag_is_a_development_build() {
            let version = version(None, Some("521c956"), CARGO);

            assert_eq!(version.identifier, "development-521c956");
            assert_eq!(version.said, "Development build, commit 521c956.");
        }

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

        #[test]
        fn a_tree_with_no_git_says_what_cargo_says() {
            let version = version(None, None, CARGO);

            assert_eq!(version.identifier, CARGO);
            assert_eq!(version.said, "Version 0.1.0.");
        }

        #[test]
        fn a_commit_that_is_not_there_is_not_a_development_build() {
            assert_eq!(version(None, Some("   "), CARGO).identifier, CARGO);
        }

        #[test]
        fn every_version_sentence_is_one_a_reader_can_speak() {
            for version in [
                version(Some("windows-v1.0.0"), Some("521c956"), CARGO),
                version(Some("windows-v1.0.0-beta"), Some("521c956"), CARGO),
                version(None, Some("521c956"), CARGO),
                version(None, None, CARGO),
            ] {
                assert!(version.said.ends_with('.'), "{}", version.said);
                assert!(!version.said.contains("  "), "{}", version.said);
                assert!(!version.identifier.trim().is_empty());
            }
        }
    }

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

    #[test]
    fn a_session_that_was_set_up_is_named_and_nothing_more_is_said_about_it() {
        let note = far_end_note(Some("bash"), SetUpOutcome::Fully);

        assert_eq!(note.said.as_deref(), Some("bash"));
        assert!(
            !note.limit_explained,
            "nothing has been said about a limit, so the grace period may still speak"
        );
    }

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

    #[test]
    fn a_shell_nobody_measured_is_named_and_said_to_be_one() {
        let note = far_end_note(Some("fish"), SetUpOutcome::NothingWritten);

        assert_eq!(
            note.said.as_deref(),
            Some("fish, which Acter cannot set up yet.")
        );
        assert!(note.limit_explained);
    }

    #[test]
    fn a_session_that_was_not_set_up_says_what_it_will_and_will_not_tell_them() {
        let note = far_end_note(Some("bash"), SetUpOutcome::Refused);

        assert_eq!(
            note.said.as_deref(),
            Some("bash. You will hear what commands print here, but not whether they worked.")
        );
        assert!(note.limit_explained);
    }

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

    #[test]
    fn a_far_end_with_nothing_to_say_says_nothing() {
        assert_eq!(far_end_note(None, SetUpOutcome::NothingWritten).said, None);
    }

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

    // Read rather than set: a variable one test sets is seen by every other test in the binary.
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

    mod the_checkbox_authorises_and_the_dialog_discloses {
        use std::sync::Mutex;

        use acter_core::{
            HostKeyAnswer, HostKeyQuestion, PasswordQuestion, ProgramAnswer, ProgramQuestion,
            Secret, SessionSetup, SshQuestions,
        };

        use super::*;

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
