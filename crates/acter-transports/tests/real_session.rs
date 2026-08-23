//! Integration test: the whole stack against a real shell, with nothing faked at all —
//! a real `LocalPty` on a real pseudoconsole, a real `AlacrittyEngine`, a real
//! `SessionService` and a real clock, observed through the `SessionEvent`s a frontend
//! would have received.
//!
//! **This is the layer the other two suites leave a gap between.** `local_pty.rs` drives
//! a real shell but only ever looks at bytes; `pipeline.rs` computes real verdicts but
//! its far end is a transcript. Neither can answer "what does a block contain when a
//! genuine shell scrolls a genuine screen", which is the whole of B4.2 and the whole of
//! manual review in an unintegrated session.
//!
//! **Every test here is `#[ignore]`d**, for the reason `local_pty.rs` gives: a workspace
//! run must spawn no process and must not depend on what is installed. Run them with:
//! `cargo test -p acter-transports --test real_session -- --ignored --nocapture`
//!
//! The nested case additionally needs Docker, and says so and returns rather than failing
//! when it is absent — a machine without Docker has not discovered a defect.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use acter_core::{
    Clock, CommandId, EventSink, Key, KeyPress, PacingConfig, SessionApi, SessionEvent, SessionId,
    SessionService, Timer,
};
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::oneshot;

/// The shell every test starts in, for `local_pty.rs`'s reasons: present everywhere,
/// starts in tens of milliseconds, and emits no shell-integration markers — which is not
/// incidental here but the precondition, since B4.2 only exists in a session nothing ever
/// tells about a command boundary.
const SHELL: &str = "cmd.exe";

/// Eighty by twenty-four, matching `pipeline.rs`, so a flood of forty rows genuinely
/// scrolls rather than merely being long.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

const SESSION: SessionId = SessionId(1);

/// Long enough that a slow machine is not what fails a test, short enough that a hang is
/// reported as one. Docker gets its own, larger, because a cold image is a real wait.
const PATIENCE: Duration = Duration::from_secs(20);
const DOCKER_PATIENCE: Duration = Duration::from_secs(90);

/// The container image the nested case runs in: tiny, and its `sh` is as plain as a
/// shell gets, so what the emulator sees is the nesting rather than a distribution's
/// prompt decoration.
const IMAGE: &str = "alpine";

/// Two hundred milliseconds rather than the shipped five seconds, for `pipeline.rs`'s
/// reason: what is under test is what a flagged session does, not how long flagging takes.
const GRACE: Duration = Duration::from_millis(200);

/// After the text a test was waiting for arrives, how long to keep listening before
/// judging a block. The defect this file exists for delivers its stale rows *interleaved*
/// with the output, so they are already present — but a block is only safe to assert
/// about once the session has stopped talking, and this is cheaper than another poll loop.
const SETTLE: Duration = Duration::from_millis(750);

/// The real clock, restated here rather than depended on: `SystemClock` lives in
/// `acter-app`, which is above this crate, and copying fifteen lines is better than a
/// dependency pointing the wrong way.
struct RealClock {
    origin: Instant,
}

impl RealClock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn timer(&self, after: Duration) -> Timer {
        let (fire, fired) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = fire.send(());
        });
        Timer::new(fired)
    }
}

/// Everything the frontend would have received.
#[derive(Default)]
struct Recorder(Mutex<Vec<SessionEvent>>);

impl EventSink for Recorder {
    fn send(&self, event: SessionEvent) {
        self.0.lock().expect("recorder poisoned").push(event);
    }
}

/// One real session, and the handle a test drives it by.
struct RealSession {
    session: SessionService,
    events: Arc<Recorder>,
}

impl RealSession {
    fn start(args: &[&str]) -> Self {
        let pty = LocalPty::spawn(SHELL, args, COLUMNS, SCREEN_LINES).expect("a shell starts");
        let events = Arc::new(Recorder::default());
        let session = SessionService::start(
            Box::new(pty),
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::new(RealClock::new()) as Arc<dyn Clock>,
            PacingConfig {
                integration_grace: GRACE,
                ..PacingConfig::default()
            },
        );
        session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);
        Self { session, events }
    }

    /// `cmd.exe` with its own echo of submitted lines off and itself kept alive after each
    /// command — `local_pty.rs`'s flags, for its reasons.
    fn cmd() -> Self {
        Self::start(&["/Q", "/K"])
    }

    fn submit(&self, line: &str) -> CommandId {
        self.session.submit_command(SESSION, line).command_id
    }

    /// Everything one block received, concatenated in order.
    fn output_of(&self, command_id: CommandId) -> String {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Output {
                    command_id: at,
                    text,
                    ..
                } if *at == command_id => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The heading one block opened with: `None` if it never opened, `Some(None)` if it
    /// opened without the far end saying what it was running.
    fn heading_of(&self, command_id: CommandId) -> Option<Option<String>> {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .find_map(|event| match event {
                SessionEvent::CommandStarted {
                    command_id: at,
                    command_line,
                } if *at == command_id => Some(command_line.clone()),
                _ => None,
            })
    }

    /// What the frontend sends for Ctrl+C: the key, not the meaning (spec B6, decision 4).
    fn ctrl_c(&self) {
        self.session.send_key(
            SESSION,
            KeyPress {
                key: Key::Char('c'),
                ctrl: true,
                shift: false,
                alt: false,
            },
        );
    }

    /// Everything the session said, whichever block it belonged to. Only ever used to
    /// make a timeout legible.
    fn rendered(&self) -> String {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Output { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Waits until `wanted` has reached `command_id`'s block, then lets the session go
    /// quiet. Panics with what the whole session said, because a timeout reading "timed
    /// out" makes a real failure and a slow machine look identical.
    async fn until(&self, command_id: CommandId, wanted: &str, patience: Duration) -> String {
        let deadline = Instant::now() + patience;
        loop {
            if self.output_of(command_id).contains(wanted) {
                tokio::time::sleep(SETTLE).await;
                return self.output_of(command_id);
            }
            if Instant::now() >= deadline {
                panic!(
                    "never saw {wanted:?} in {command_id:?}. What the session said was:\n{}",
                    self.rendered()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Waits out the integration grace period, so a submission lands in a session that has
    /// already been flagged rather than one still deciding. Production submits whenever the
    /// user does; the tests below are about what happens *after* flagging, and this keeps
    /// that from depending on how fast the machine got to the first `submit`.
    async fn flagged(&self) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if self
                .events
                .0
                .lock()
                .expect("recorder poisoned")
                .contains(&SessionEvent::IntegrationUnavailable)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("a marker-less shell was never flagged as unintegrated");
    }
}

/// What each test floods the screen with, and what it runs afterwards. The rows are
/// prefixed so that finding one in the wrong block is unambiguous rather than a guess
/// about whose "line 3" it was.
const ROWS: u32 = 40;
const ROW_PREFIX: &str = "acter-row-";
const AFTER: &str = "acter-second-block";

/// Asserts the whole of B4.2 against a block: it has its own output, and it has nothing
/// of the flood that ran before it.
fn only_its_own(after: &str) {
    assert!(
        after.contains(AFTER),
        "the second block has its own output: {after:?}"
    );
    assert!(
        !after.contains(ROW_PREFIX),
        "and not one row of the command that scrolled away under it: {after:?}"
    );
}

/// B4.2 end to end, with no fake anywhere in the stack.
///
/// A real `cmd.exe` prints forty rows onto a twenty-four row screen and finishes. Its last
/// screenful is still live in the extractor, and nothing has told the extractor otherwise:
/// `place` calls `settle_block` only on an OSC 133 `D`, which `cmd.exe` never sends. The
/// user then runs something else, and every row that command prints pushes one of the
/// flood's rows above the view, where it is re-emitted complete.
///
/// Before the fix this block contained the tail of the flood interleaved with its own
/// output, one stale row per new row — which is the shape of the capture that opened the
/// entry, and which makes a block unreadable to someone reviewing it a line at a time.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_shells_flood_does_not_scroll_into_the_next_block() {
    let session = RealSession::cmd();
    session.flagged().await;

    let flood = session.submit(&format!(
        "for /l %i in (1,1,{ROWS}) do @echo {ROW_PREFIX}%i"
    ));
    session
        .until(flood, &format!("{ROW_PREFIX}{ROWS}"), PATIENCE)
        .await;

    let next = session.submit(&format!("echo {AFTER}"));
    let after = session.until(next, AFTER, PATIENCE).await;

    only_its_own(&after);
}

/// The same thing one shell deeper, which is the case with no way back: a shell running
/// inside the shell Acter started.
///
/// It matters because it is the regime the defect lives in and cannot leave. Markers are
/// injected into the shell Acter launches, so B5 can make *that* shell integrated — but a
/// container's shell is on the far side of that injection point and can never be reached
/// by it. A nested session is therefore permanently unintegrated, permanently subject to
/// this bug, and permanently dependent on manual buffer review, which is exactly what a
/// mangled buffer destroys.
///
/// Docker is the honest version rather than `cmd.exe` inside `cmd.exe`, because it changes
/// everything the emulator has to survive at once: a Linux pty inside a Windows
/// pseudoconsole, bare line feeds instead of carriage-return pairs, and a prompt drawn by
/// something that has never heard of the outer shell.
#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn a_shell_inside_a_shell_does_not_mangle_the_buffer() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    let session = RealSession::cmd();
    session.flagged().await;

    // Entering the container is itself a command, and an unintegrated session gives it a
    // block. Everything after this lands in blocks whose output was produced by a shell
    // Acter never launched and cannot instrument.
    //
    // Waiting for the container's own prompt rather than for an `echo` of our own is what
    // keeps the outer `cmd.exe` from answering by mistake: until `sh` has drawn `/ #`,
    // a line submitted here could still be read by the shell we came from, and its reply
    // would look exactly like the container's.
    let enter = session.submit(&format!("docker run -it --rm {IMAGE} sh"));
    session.until(enter, "/ #", DOCKER_PATIENCE).await;

    let flood = session.submit(&format!(
        "i=1; while [ $i -le {ROWS} ]; do echo {ROW_PREFIX}$i; i=$((i+1)); done"
    ));
    session
        .until(flood, &format!("{ROW_PREFIX}{ROWS}"), DOCKER_PATIENCE)
        .await;

    let next = session.submit(&format!("echo {AFTER}"));
    let after = session.until(next, AFTER, DOCKER_PATIENCE).await;

    only_its_own(&after);

    session.submit("exit");
}

/// Whether this machine can run the nested case at all. Deliberately a check on the
/// daemon rather than on the client: `docker` being on `PATH` with nothing behind it is
/// the common shape of "installed", and it would hang the test rather than skip it.
fn docker_is_available() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"])
}

/// Pulls the image before the session starts, so a cold machine waits here rather than
/// inside the emulator. A pull's progress bars are carriage-return animations, and having
/// them land in a block would make this a test of two things at once.
fn image_is_present() -> bool {
    docker(&["pull", "--quiet", IMAGE])
}

fn docker(args: &[&str]) -> bool {
    std::process::Command::new("docker")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// B4.4, against a real `cmd.exe`: the far end's echo opens the block and becomes its
/// heading, rather than arriving under the heading as the first thing the command printed.
///
/// This is the duplication a listener actually hits — measured through NVDA on
/// 2026-08-22, every block showed the submitted line twice — and it cannot be pinned
/// anywhere below this file, because it depends on ConPTY really echoing what was written
/// to it onto the row the prompt was drawn on.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_shells_echo_opens_the_block_and_becomes_its_heading() {
    let session = RealSession::cmd();
    session.flagged().await;

    let line = format!("echo {AFTER}");
    let command = session.submit(&line);
    let output = session.until(command, AFTER, PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some(line.clone())),
        "the heading is the echo the shell produced: {:?}",
        session.heading_of(command)
    );
    assert!(
        !output.contains(&line),
        "and the command line is not also the block's first content line: {output:?}"
    );
}

/// 22.10, folded into B4.4: a backlog released all at once gets one block per submission,
/// each holding its own output.
///
/// Before this, `ping` held the console without reading it, the two queued lines produced
/// two headings with nothing under them, and the interrupt released both at once into
/// whichever block happened to be open last — measured 2026-08-22 as two empty blocks and
/// a third holding two commands' echoes and output. Windows does not discard typed-ahead
/// input on Ctrl+C and a real `cmd.exe` behaves the same way, so what is fixed here is the
/// shape rather than the ordering: a block opens when the far end echoes a line, so the
/// blocks appear when the lines actually run.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_backlog_released_by_an_interrupt_fills_its_own_blocks() {
    let session = RealSession::cmd();
    session.flagged().await;

    let held = session.submit("ping -n 20 127.0.0.1");
    session.until(held, "Pinging", PATIENCE).await;

    let queued: Vec<CommandId> = (1..=2)
        .map(|n| session.submit(&format!("echo acter-backlog-{n}")))
        .collect();
    tokio::time::sleep(Duration::from_millis(750)).await;

    session.ctrl_c();
    session.until(queued[1], "acter-backlog-2", PATIENCE).await;

    for (index, command) in queued.iter().enumerate() {
        let wanted = format!("acter-backlog-{}", index + 1);
        assert!(
            session.output_of(*command).contains(&wanted),
            "each released submission holds its own output, not its sibling's: \
             {wanted} was not in {:?}",
            session.output_of(*command)
        );
    }
}
