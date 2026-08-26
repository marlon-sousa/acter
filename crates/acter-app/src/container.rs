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
use std::sync::Arc;

use acter_core::{
    Clock, ConnectApi, ConnectService, PacingConfig, ProfileId, SessionApi, SessionFactory,
    SessionService, ShellAdapter, ShellFacts, SshQuestions, Transport, Unasked,
};
use acter_shells::{Plain, ThisMachine, Wsl, adapter_for};
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, LocalPty, ScriptedTransport, SessionTranscript, TranscriptShell, Unmarked,
};
use tauri::{Builder, generate_context, generate_handler};

use crate::adapters::SystemClock;
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

    let builder = Builder::default()
        .manage(state)
        .invoke_handler(generate_handler![
            crate::routers::attach_session,
            crate::routers::submit_command,
            crate::routers::send_key,
            crate::routers::about,
            crate::routers::platform,
            crate::routers::connectable,
            crate::routers::use_profile,
            crate::routers::answer_connect,
            crate::routers::attempt_ended,
            crate::routers::connected
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
    let service = Arc::new(state());
    if let Some(profile) = launch_profile() {
        // Nothing reads the error here on purpose: `connected()` answers `None`, which is
        // the unconnected window, and the frontend says so. Reporting the reason as well is
        // B8's, where a `--profile` that resolves to nothing has to name what was asked for.
        // Nobody to ask at launch: there is no window yet, so a far end that needs a
        // decision is refused rather than trusted (`Unasked`).
        let _ = service.use_profile(&profile, &(Arc::new(Unasked) as Arc<dyn SshQuestions>));
    }
    let connect = Arc::clone(&service) as Arc<dyn ConnectApi>;
    AppState {
        session: Arc::clone(&service) as Arc<dyn SessionApi>,
        connecting: Arc::new(Connecting::new(Arc::clone(&connect))),
        connect,
    }
}

/// The connect service, wired and empty — the shape every test of the invoke surface wants.
pub(crate) fn state() -> ConnectService {
    ConnectService::new(
        Arc::new(Shells::new()),
        Arc::new(ThisMachine::new()),
        scripted_profiles(),
    )
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
}

impl Shells {
    fn new() -> Self {
        Self {
            clock: Arc::new(SystemClock::new()),
        }
    }

    /// One session over a far end, whatever that far end is.
    fn session(
        &self,
        transport: Box<dyn Transport>,
        shell: &dyn ShellAdapter,
    ) -> Arc<dyn SessionApi> {
        Arc::new(SessionService::start(
            transport,
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::clone(&self.clock),
            PacingConfig::default(),
            ShellFacts::of(shell),
        ))
    }

    /// A real shell on a pseudoconsole.
    ///
    /// The adapter and the far end are one decision, which is why they travel together:
    /// injecting cmd's prompt markers without telling the domain that this shell emits no
    /// `C` produces a session that receives markers, opens no block and speaks nothing at
    /// all — measured before either half was written (ROADMAP 22.5).
    fn real(&self, shell: Box<dyn ShellAdapter>) -> Result<Arc<dyn SessionApi>, String> {
        let launch = shell.launch();
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        let environment: Vec<(&str, &str)> = launch
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)?;
        Ok(self.session(Box::new(pty), shell.as_ref()))
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
        profile: &ProfileId,
        _questions: &Arc<dyn SshQuestions>,
    ) -> Result<Arc<dyn SessionApi>, String> {
        let runtime = tauri::async_runtime::handle();
        let _entered = runtime.inner().enter();
        self.start(profile).map_err(ended)
    }
}

impl Shells {
    /// The branch per kind of profile, with the runtime already entered.
    fn start(&self, profile: &ProfileId) -> Result<Arc<dyn SessionApi>, String> {
        match profile {
            // The composition root names no shell since B5.1: which shell this is, what it
            // is started with and what it can mark are one object's answers, and the same
            // object answers them for the suites that measure a real one.
            ProfileId::Shell { kind } => self.real(adapter_for(kind.program())),
            ProfileId::Distribution { name } => {
                self.real(Box::new(Wsl::in_distribution(WSL_CLIENT, name)))
            }
            // Trimmed for the reason A9 found: `set ACTER_SHELL=x && acter` in cmd puts
            // everything up to the `&&` into the value, trailing space included — and a
            // name with a stray space matches no adapter, so the session silently became an
            // unintegrated shell with no injection, no markers and no prompt. A shell nobody
            // recognises is a real state this product supports; being pushed into it by
            // punctuation is not.
            ProfileId::Program { program } => self.real(adapter_for(program.trim())),
            #[cfg(debug_assertions)]
            ProfileId::Scripted { name } => self.scripted(name),
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
    use super::*;

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
}
