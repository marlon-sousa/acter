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

use std::env;
use std::sync::Arc;

use acter_core::{
    Clock, PacingConfig, SessionApi, SessionService, ShellAdapter, ShellFacts, Transport,
};
use acter_shells::Plain;
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, LocalPty, ScriptedTransport, SessionTranscript, TranscriptShell, Unmarked,
};
use tauri::{Builder, generate_context, generate_handler};

use crate::adapters::SystemClock;

/// The environment variable choosing which simulated session to run: a built-in name, or
/// a path to a transcript JSON. Unset means the built-in transcript, read whole.
const TRANSCRIPT_ENV: &str = "ACTER_TRANSCRIPT";

/// The environment variable naming a real shell to run instead of a scripted session:
/// a program for `LocalPty` to spawn, `cmd.exe` or `powershell.exe` for instance. Unset
/// runs the scripted far end, which is still what an ordinary launch gets.
///
/// **Not the profile machinery, and not convergence** (spec B4, decision 9). Choosing a
/// transport by profile, and making a real one the default, is convergence's. What this
/// buys now is that B4 can be *heard*: a manual accessibility run needs a way to say
/// which session it is testing, which is the same reason `ACTER_TRANSCRIPT` exists.
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
const SHELL_ENV: &str = "ACTER_SHELL";

/// The emulated screen the engine keeps. Eighty by twenty-four, the same as the
/// automated suite uses, so a manual session scrolls where the tests say it scrolls.
/// A real transport is opened at this size too, so the emulator and the far end agree
/// from the first byte; a window-driven resize is still nobody's yet.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

pub(crate) struct AppState {
    pub(crate) session: Arc<dyn SessionApi>,
}

pub fn run() {
    // The session starts two tasks and arms a timer, so it is built inside the runtime
    // Tauri owns rather than beside it. Entering that runtime is the composition root's
    // business by definition: it is the one place that knows a runtime exists at all.
    let runtime = tauri::async_runtime::handle();
    let state = {
        let _entered = runtime.inner().enter();
        AppState {
            session: Arc::new(session()),
        }
    };

    let builder = Builder::default()
        .manage(state)
        .invoke_handler(generate_handler![
            crate::routers::attach_session,
            crate::routers::submit_command,
            crate::routers::send_key,
            crate::routers::about,
            crate::routers::platform
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

/// The one session: a far end on a real pipeline.
pub(crate) fn session() -> SessionService {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let (transport, shell) = transport(Arc::clone(&clock));
    SessionService::start(
        transport,
        Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
        clock,
        PacingConfig::default(),
        ShellFacts::of(shell.as_ref()),
    )
}

/// Which far end this launch talks to: a real shell if `ACTER_SHELL` names one, and the
/// scripted session otherwise.
///
/// A shell that cannot be started is a loud failure carrying the sentence `LocalPty`
/// wrote, for the same reason a transcript that cannot be loaded is one: a window that
/// opens onto a session that will never say anything is worse than one that does not
/// open, and worst of all for a user who cannot see that it is empty.
/// It returns the shell alongside the far end itself, because the two are one decision:
/// injecting cmd's prompt markers without telling the domain that this shell emits no `C`
/// produces a session that receives markers, opens no block and speaks nothing at all —
/// measured before either half was written (ROADMAP 22.5). Since B5.2 the session needs two
/// answers from a shell rather than one — how far its markers reach, and what ends its
/// input — so this hands the adapter over whole and lets the caller take what it needs out
/// of it as [`ShellFacts`].
///
/// A scripted far end is a shell Acter knows nothing about, which is what [`Plain`] is:
/// the transcript speaks the markers itself, and nothing is injected into a fake.
///
/// [`Plain`]: acter_shells::Plain
fn transport(clock: Arc<dyn Clock>) -> (Box<dyn Transport>, Box<dyn ShellAdapter>) {
    match env::var(SHELL_ENV) {
        Ok(program) => {
            // The composition root names no shell since B5.1: which shell this is, what it
            // is started with and what it can mark are one object's answers, and the same
            // object answers them for the suites that measure a real one.
            let adapter = acter_shells::adapter_for(&program);
            let launch = adapter.launch();
            let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
            let environment: Vec<(&str, &str)> = launch
                .environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
                .unwrap_or_else(|why| panic!("{why}"));
            (Box::new(pty), adapter)
        }
        Err(_) => {
            let (shell, chunking) = far_end();
            (
                Box::new(ScriptedTransport::with_shell(shell, chunking, clock)),
                Box::new(Plain::new(SCRIPTED)),
            )
        }
    }
}

/// What a scripted session is started as, for the one place a name is still needed: the
/// null adapter carries the program it was selected by, and no process is ever spawned
/// from this one.
const SCRIPTED: &str = "scripted";

/// Resolves `ACTER_TRANSCRIPT` to a far end and a delivery strategy.
///
/// **A name resolves to a composition, not to a file** (spec B6, decision 12, as B3.6
/// left it). Two of the three simulated shells worth naming stopped being transcripts:
/// "never emits markers" is a decorator over any far end, and "splits a marker across two
/// reads" is a property of the pipe carrying any transcript at all. So the table below is
/// the product of a shell and a chunking rather than a list of files somebody wrote, and
/// a path still names a transcript for everything that genuinely is one — a forged
/// marker, for instance, which is something a *program* does.
///
/// Reading the environment and the filesystem is the container's privilege: this is the
/// composition root, where the world is allowed in. A name that resolves to nothing is a
/// loud startup failure naming what was asked for, never a silent fallback to something
/// else — a manual accessibility run that quietly tested the wrong session would be worse
/// than one that did not start.
fn far_end() -> (Box<dyn FakeShell>, Chunking) {
    let Ok(named) = env::var(TRANSCRIPT_ENV) else {
        return (Box::new(TranscriptShell::builtin()), Chunking::Whole);
    };
    match named.as_str() {
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
            Box::new(TranscriptShell::new(transcript(path))),
            Chunking::Whole,
        ),
    }
}

fn transcript(path: &str) -> SessionTranscript {
    SessionTranscript::load(path).unwrap_or_else(|why| {
        panic!(
            "Acter could not start the session {TRANSCRIPT_ENV} asked for. {path} is not \
             one of the built-in names (builtin, builtin-by-byte, unmarked, \
             unmarked-by-byte), and it could not be loaded as a transcript file: {why}"
        )
    })
}
