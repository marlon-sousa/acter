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
//! when it is absent — a machine without Docker has not discovered a defect. The WSL group
//! at the end of this file skips the same way and for the same reason, asking the
//! `InstalledShells` port whether there is a distribution rather than whether `wsl.exe`
//! exists: every Windows 11 install ships that binary, and a client with nothing behind it
//! would hang waiting for a prompt that never comes instead of skipping.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use acter_core::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, InstalledShells, Key,
    KeyAck, KeyPress, PacingConfig, SessionApi, SessionEvent, SessionId, SessionService,
    ShellAdapter, ShellFacts, ShellLaunch, ShellMarkers, SubmitAck, Timer,
};
use acter_shells::ThisMachine;
use acter_term::AlacrittyEngine;
use acter_transports::LocalPty;
use tokio::sync::oneshot;

/// The shell every test starts in, for `local_pty.rs`'s reasons: present everywhere,
/// starts in tens of milliseconds, and emits no shell-integration markers — which is not
/// incidental here but the precondition, since B4.2 only exists in a session nothing ever
/// tells about a command boundary.
const SHELL: &str = "cmd.exe";

/// The shell the marker tests below are about: Windows PowerShell 5.1, which is on every
/// Windows machine including the CI image. PowerShell 7 is the other edition and is
/// deliberately not what this suite drives — it is installed separately, so a suite that
/// needed it would be red on a machine that simply does not have it, and B5.2 measured the
/// two producing byte-for-byte the same marker stream.
const POWERSHELL: &str = "powershell.exe";

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
    /// A session over a shell started exactly as this launch says, declaring this much
    /// about its own markers. The launch and the markers travel together because they are
    /// one decision: injecting cmd's prompt markers without telling the domain the shell
    /// emits no `C` produces a session that receives markers and speaks nothing
    /// (spec B4.5) — which is why since B5.1 both come from one adapter rather than from
    /// two arguments this file assembles.
    fn over(launch: &ShellLaunch, markers: ShellMarkers) -> Self {
        Self::over_within(launch, markers, GRACE)
    }

    /// A session over the shell this adapter describes, started and declared exactly as
    /// the application starts and declares it — launch, markers and end-of-input all taken
    /// from the one object, so nothing here can drift from what ships.
    ///
    /// The grace period is the caller's because it is the one thing that is *not* the
    /// shell's: see [`Self::powershell`], which is the only caller that has to state it.
    fn adapted(shell: &dyn ShellAdapter, grace: Duration) -> Self {
        Self::launched(&shell.launch(), ShellFacts::of(shell), grace)
    }

    /// The same, with the integration grace period stated rather than accelerated.
    ///
    /// Most of this file is about what a *flagged* session does, so it shortens the grace
    /// to two hundred milliseconds and gets there fast. A session that is supposed to
    /// become integrated cannot use that: starting a WSL distribution takes seconds, the
    /// first marker arrives after them, and a two-hundred-millisecond grace flags the
    /// session before bash has drawn a prompt. That is the shipped behaviour and not a
    /// defect — DESIGN decision 8 upgrades such a session silently the moment a marker
    /// arrives — but it makes "was this session ever flagged" a question about the clock
    /// rather than about the shell.
    fn over_within(launch: &ShellLaunch, markers: ShellMarkers, grace: Duration) -> Self {
        Self::launched(launch, ShellFacts { markers, eof: None }, grace)
    }

    /// The one place a real session is actually built, however the caller described the
    /// shell it is over.
    fn launched(launch: &ShellLaunch, shell: ShellFacts, grace: Duration) -> Self {
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        let environment: Vec<(&str, &str)> = launch
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
            .expect("a shell starts");
        let events = Arc::new(Recorder::default());
        let session = SessionService::start(
            Box::new(pty),
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::new(RealClock::new()) as Arc<dyn Clock>,
            PacingConfig {
                integration_grace: grace,
                ..PacingConfig::default()
            },
            shell,
        );
        session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);
        Self { session, events }
    }

    /// `cmd.exe` started the way the application starts it, but told nothing about its
    /// markers and given no injection: the unintegrated session most of this file is
    /// about, and the precondition B4.2 only exists under.
    ///
    /// The arguments come from the adapter since B5.1, so this suite and the product
    /// cannot drift into measuring different streams; the injection is dropped
    /// deliberately, which is the one thing that separates this from [`Self::marked`].
    fn cmd() -> Self {
        let launch = ShellLaunch {
            environment: Vec::new(),
            ..acter_shells::adapter_for(SHELL).launch()
        };
        Self::over(&launch, ShellMarkers::Full)
    }

    /// The same shell as the application runs it, integration and all: the launch and the
    /// markers exactly as `acter_shells` states them.
    fn marked() -> Self {
        let adapter = acter_shells::adapter_for(SHELL);
        Self::over(&adapter.launch(), adapter.markers())
    }

    fn submit(&self, line: &str) -> CommandId {
        // The one place in these suites that reads an ack apart: a running session always
        // accepts, and the other answer belongs to a window with no session at all.
        match self.session.submit_command(SESSION, line) {
            SubmitAck::Accepted { command_id } => command_id,
            SubmitAck::NotConnected => panic!("a running session accepts a line"),
        }
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
    /// **The one thing it does not take from the rest of this file is the grace period,
    /// and that is a measurement rather than a convenience.** Every other session here runs
    /// under two hundred milliseconds because `cmd.exe` draws its prompt in tens of them;
    /// PowerShell does not, and measured 2026-08-24 a PowerShell session under that grace
    /// is flagged `IntegrationUnavailable` before its first marker has arrived. That is
    /// correct behaviour and it recovers silently (DESIGN decision 8), but it is a fact
    /// about how long a shell takes to start rather than about its markers, so this runs
    /// under the grace the product ships — which is twenty-five times longer and never
    /// close.
    fn powershell() -> Self {
        Self::adapted(
            acter_shells::adapter_for(POWERSHELL).as_ref(),
            PacingConfig::default().integration_grace,
        )
    }

    /// What the frontend would send for Ctrl+D: end of input, whatever that costs in
    /// bytes for this shell (spec B5.2, decision 5).
    fn ctrl_d(&self) -> KeyAck {
        self.session.send_key(
            SESSION,
            KeyPress {
                key: Key::Char('d'),
                ctrl: true,
                shift: false,
                alt: false,
            },
        )
    }

    /// Whether the block closed because the shell said it had — the marker cmd cannot
    /// send, and therefore the fact this suite could not assert before PowerShell.
    fn finished(&self, command_id: CommandId) -> bool {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .any(|event| {
                matches!(
                    event,
                    SessionEvent::CommandFinished { command_id: at } if *at == command_id
                )
            })
    }

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

    /// Waits until everything the session has said satisfies `wanted`, then lets it go
    /// quiet. For the cases with no block to wait on: a bare Enter is answered by the
    /// shell drawing its prompt again, which belongs to whichever block happens to be
    /// open.
    async fn until_said(&self, patience: Duration, wanted: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + patience;
        loop {
            if wanted(&self.rendered()) {
                tokio::time::sleep(SETTLE).await;
                return self.rendered();
            }
            if Instant::now() >= deadline {
                panic!(
                    "the session never said it. What it did say was:\n{}",
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

/// A far end that asks the terminal where the cursor is and then does not read the
/// answer. Six seconds of sleeping is all it takes; the container that held a tty in
/// the original capture was incidental (ROADMAP 22.11).
const SLOW_CONSUMER: &str = concat!(
    r#"powershell -NoProfile -Command "[Console]::Write([char]27 + '[6n'); "#,
    r#"Start-Sleep -Seconds 6""#
);

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

/// A real `cmd.exe` with its `PROMPT` carrying OSC 133 `A` and `B` — the whole of 22.5
/// against the shell it is about (spec B4.5, decisions 1 to 4).
///
/// **The measurement this replaces is the reason both halves are one PR.** Setting the
/// variable against the domain as it stood did not degrade such a session, it deleted it:
/// no `CommandStarted`, no output, nothing spoken, because `BlockStarted` came only from a
/// `C` and `wants` accepted only `Output`.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_cmd_carries_its_own_prompt_markers() {
    let session = RealSession::marked();

    let command = session.submit("echo acter-marked-line");
    let output = session.until(command, "acter-marked-line", PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some("echo acter-marked-line".to_owned())),
        "the block is named by what the far end echoed, and the echo is what opened it"
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a shell whose prompt carries A and B is integrated, not flagged"
    );
    assert!(
        output.contains('>'),
        "the returning prompt is the last thing the block says — the only ending a shell \
         with no exit code has: {output:?}"
    );
}

/// 22.11: a device-query answer the program that asked never read, and the submitted line
/// that used to be concatenated onto it.
///
/// The far end is a slow consumer — it writes a cursor-position query and then sleeps
/// without reading — which is all it takes. The tty-holding container the defect was found
/// in was incidental to the capture, not a precondition.
///
/// Before decision 7 the next submission came back as
/// `'s not recognized as an internal or external command,`, naming a command the user
/// never typed.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_submission_behind_an_unread_device_query_answer_still_runs() {
    let session = RealSession::marked();

    let slow = session.submit(SLOW_CONSUMER);
    // Long enough for the query to be answered and for the far end to give up without
    // reading: the sleep is six seconds.
    session.until(slow, ">", DOCKER_PATIENCE).await;

    let command = session.submit("echo acter-second-line");
    let output = session.until(command, "acter-second-line", PATIENCE).await;

    assert!(
        !session.rendered().contains("not recognized"),
        "the line the user submitted is the line the shell ran: {output:?}"
    );
    assert!(
        !session.rendered().contains("^["),
        "and this pump's own answer is never in the buffer as text: {:?}",
        session.rendered()
    );
}

/// **Every** command in a marked cmd session, not only the first: the block holds the
/// command's output and the returning prompt, and never the line the user typed.
///
/// Written during the NVDA pass for this spec, which heard exactly that failure — the
/// first command clean and every one after it reading the user's own typing back, glued to
/// the answer as `echo bravobravo`. It turned out to be a session that was never
/// integrated at all, so what was heard is ROADMAP 22.12 in an unintegrated session rather
/// than anything about markers. The test stays because nothing else asserted that the
/// exclusion holds past the *first* command, which is precisely where 22.12 says an
/// unmarked session stops holding it.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_a_marked_session_reads_the_typed_line_back() {
    let session = RealSession::marked();

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    let output = session.until(second, "acter-bravo", PATIENCE).await;

    // Asked of the whole session, not of one block: the echo was reaching the buffer as
    // output of the *previous* command, which an assertion about this block cannot see.
    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo"),
        "the command line is never read back at the user, in any block: {said:?}"
    );
    assert!(
        output.contains('>'),
        "and the returning prompt is still the last thing it says: {output:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

/// **B4.9 against the session the marker work cannot reach**, and the one a listener met
/// first: an unintegrated `cmd.exe`, where every region is `Unstructured` and the only
/// thing separating the echo from output is where the far end wrote it.
///
/// B4.4 fixed the first command of such a session, because its echo is held for want of a
/// block and dropped. Every command after it had a block open, so the echo went straight
/// into it — measured through NVDA on 2026-08-22 as the user's own typing read back before
/// every answer.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_an_unintegrated_session_reads_the_typed_line_back() {
    let session = RealSession::cmd();
    session.flagged().await;

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    let output = session.until(second, "acter-bravo", PATIENCE).await;

    // Asked of the whole session rather than of one block: the echo reached the buffer as
    // output of the *previous* command, which an assertion about this block cannot see.
    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo") && !said.contains("echo acter-alpha"),
        "no command line is read back at the user, in any block: {said:?}"
    );
    assert!(
        output.contains('>'),
        "and the returning prompt is still the last thing it says: {output:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

/// **The case B4.5 could not reach, which is why this rule is positional and not a
/// marker.** Inside `docker run -it` the proxying command's `C..D` never closes, so
/// everything the container writes lands in that one open block — the echo of every line
/// typed into it included, before the boundary has recognised it.
///
/// Found by the user on 2026-08-23, in B4.5's manual pass: the outer shell had just been
/// made clean by the markers, and the container read every line back.
#[tokio::test]
#[ignore = "spawns a real shell and a container"]
async fn a_line_typed_into_a_container_is_not_read_back() {
    if !docker_is_available() || !image_is_present() {
        println!("skipped: Docker is not available on this machine");
        return;
    }

    let session = RealSession::marked();

    // Waiting for the container's own prompt rather than for an echo of our own is what
    // keeps the outer `cmd.exe` from answering by mistake: until `sh` has drawn `/ #`, a
    // line submitted here could still be read by the shell we came from.
    let enter = session.submit(&format!("docker run -it --rm {IMAGE} sh"));
    session.until(enter, "/ #", DOCKER_PATIENCE).await;

    let inside = session.submit(&format!("echo {AFTER}"));
    session.until(inside, AFTER, DOCKER_PATIENCE).await;

    let said = session.rendered();
    assert!(
        !said.contains(&format!("echo {AFTER}")),
        "the line typed into the container is not read back at the user: {said:?}"
    );
    assert!(
        said.contains("/ #"),
        "and the container's own prompt still is: {said:?}"
    );

    session.submit("exit");
}

/// The second half of 22.12: a bare Enter reaches the far end, and the shell answers it
/// with the prompt.
///
/// It used to be dropped in the frontend, so no bytes were written, the shell had no
/// reason to redraw anything and the user heard nothing at all — which in a session whose
/// only ending is the returning prompt leaves them with no way to ask where they are.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_bare_enter_brings_the_prompt_back() {
    let session = RealSession::cmd();
    session.flagged().await;

    let first = session.submit("echo acter-alpha");
    let before = session
        .until(first, "acter-alpha", PATIENCE)
        .await
        .matches('>')
        .count();

    session.submit("");

    let said = session
        .until_said(PATIENCE, |said| said.matches('>').count() > before)
        .await;
    assert!(
        said.matches('>').count() > before,
        "the shell drew its prompt again, and the user hears where they are: {said:?}"
    );
}

/// The WSL client, named the way a user would name it. Which distribution that reaches is
/// WSL's own default, which is deliberate: naming one here would test a machine's
/// configuration rather than the adapter.
const WSL: &str = "wsl.exe";

/// Long enough for a cold distribution to be started by the WSL service, which is a real
/// wait the first time and nothing afterwards.
const WSL_PATIENCE: Duration = Duration::from_secs(60);

impl RealSession {
    /// Bash inside a WSL distribution, launched and declared exactly as the application
    /// launches and declares it: the `-d`-less launch, the two-variable injection, and the
    /// full marker cycle. Everything in this file's WSL group asks the adapter rather than
    /// spelling anything out, so the suite and the product cannot measure different
    /// streams (spec B5.1, decision 5).
    fn wsl() -> Self {
        let adapter = acter_shells::adapter_for(WSL);
        Self::over_within(
            &adapter.launch(),
            adapter.markers(),
            PacingConfig::default().integration_grace,
        )
    }

    /// The verdict about one block, if the session reached one. `Some(code)` is a failure
    /// announced with the code it failed with; `None` is a block that ended without one,
    /// which for a successful command is the only thing on the wire (A6, decision 2).
    fn failure_of(&self, command_id: CommandId) -> Option<ExitCode> {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .find_map(|event| match event {
                SessionEvent::Announce {
                    command_id: at,
                    announcement: Announcement::Failed { exit_code },
                } if *at == command_id => Some(*exit_code),
                _ => None,
            })
    }

    /// Whether one block ended, by either of the two endings a marked session has.
    fn ended(&self, command_id: CommandId) -> bool {
        self.events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .any(|event| match event {
                SessionEvent::CommandFinished { command_id: at }
                | SessionEvent::CommandInterrupted { command_id: at } => *at == command_id,
                _ => false,
            })
    }

    /// Waits until `wanted` has reached `command_id`'s block, with WSL's patience.
    async fn wsl_until(&self, command_id: CommandId, wanted: &str) -> String {
        self.until(command_id, wanted, WSL_PATIENCE).await
    }
}

/// Whether this machine has a WSL distribution to start at all, asked through the port
/// that exists to answer it.
///
/// A machine without WSL has not discovered a defect, so these skip and say so, the way
/// the Docker cases do. Deliberately the full question rather than "is `wsl.exe` on
/// `PATH`": every Windows 11 install ships that binary, and a machine with the client and
/// no distribution would hang on a prompt that never comes rather than skip.
fn wsl_is_available() -> bool {
    ThisMachine::new().wsl_distributions().is_ok()
}

/// **The whole of B5.3 in one session**: a real bash, in a real distribution, reached
/// through a real `wsl.exe`, marking every boundary of a command it ran.
///
/// This is the first shipped shell that emits `C` and `D`, so it is the first session
/// where a block genuinely opens where output begins and genuinely closes when the command
/// ends, rather than being reconstructed from an echo and a returning prompt. What makes
/// it possible is that bash inherits the environment before it sources `.bashrc`, so a
/// `PROMPT_COMMAND` crossed with `WSLENV` runs *after* the user's own configuration and
/// wraps the prompt that file chose instead of losing to it.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_real_bash_under_wsl_marks_the_boundaries_of_the_command_it_ran() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl();

    let line = "echo acter-under-wsl";
    let command = session.submit(line);
    let output = session.wsl_until(command, "acter-under-wsl").await;

    assert_eq!(
        session.heading_of(command),
        Some(Some(line.to_owned())),
        "the block is named by the line bash echoed between B and C"
    );
    assert!(
        output.contains("acter-under-wsl"),
        "and holds what the command printed: {output:?}"
    );
    assert!(
        !output.contains(line),
        "the command line belongs in the heading and nowhere else: {output:?}"
    );
    assert!(
        session.ended(command),
        "a shell that emits D closes its own block, rather than leaving it open until \
         the next prompt"
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a session whose PROMPT_COMMAND survived the user's .bashrc is integrated, and \
         within the grace period the application really ships"
    );
}

/// **The first verdict any shipped session has had.** cmd cannot say how a command went,
/// so `Announce { Failed }` had never been produced by anything but a transcript; bash
/// puts the exit status in its `D` marker and this is where that becomes real.
///
/// A listener hears the difference directly: until now the returning prompt was the only
/// ending a session offered, and "it failed, with code three" was a sentence the product
/// could form and never had cause to.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_command_that_fails_under_wsl_is_announced_with_the_code_it_failed_with() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl();

    // A first command establishes the session and proves the markers are flowing, so a
    // failure to see the verdict below cannot be "bash never started".
    let opener = session.submit("echo acter-before-the-failure");
    session.wsl_until(opener, "acter-before-the-failure").await;
    assert_eq!(
        session.failure_of(opener),
        None,
        "a command that succeeded is not announced as a failure"
    );

    let failing = session.submit("(exit 3)");
    session
        .until_said(WSL_PATIENCE, |_| session.failure_of(failing).is_some())
        .await;

    assert_eq!(
        session.failure_of(failing),
        Some(ExitCode(3)),
        "the code bash reported in its D marker is the code the user is told"
    );
}

/// **The interrupt, pinned as a test rather than as a probe.** 22.6 measured that `0x03`
/// crosses `wsl.exe` into the distribution and that bash's own line discipline turns it
/// into `SIGINT`; this is the first shipped shell where it is the *product* relying on
/// that rather than an investigation.
///
/// `sleep` in a loop rather than a single long `sleep`, because what has to survive is a
/// shell that is running something, not a shell waiting on one syscall.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn an_interrupt_stops_a_program_inside_a_wsl_distribution() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    let session = RealSession::wsl();

    let opener = session.submit("echo acter-before-the-interrupt");
    session
        .wsl_until(opener, "acter-before-the-interrupt")
        .await;

    let held = session.submit("while true; do sleep 1; done");
    // Nothing is printed by a silent loop, so there is no text to wait for: this is long
    // enough that bash has certainly read the line and started running it.
    tokio::time::sleep(Duration::from_secs(2)).await;

    session.ctrl_c();

    let after = session.submit("echo acter-still-alive");
    let output = session.wsl_until(after, "acter-still-alive").await;

    assert!(
        session.ended(held),
        "the loop's block was closed rather than left open forever"
    );
    assert!(
        output.contains("acter-still-alive"),
        "and the session goes on working afterwards: {output:?}"
    );
}

/// **Nothing is written into the distribution**, which is the pinned constraint of the
/// whole entry (spec B5.3, decision 2): a WSL adapter that appended to `.bashrc` or
/// dropped an init file into the user's home would be easier and would change a machine
/// the user has to live in after Acter is closed.
///
/// Asked from inside the integrated session itself, and compared with the same question
/// put to a `wsl.exe` Acter never touched. Deliberately about the startup files rather
/// than about the home directory to the byte: bash appends to `.bash_history` whenever a
/// person uses it, which is bash's business and not Acter's.
#[tokio::test]
#[ignore = "spawns a real shell and needs a WSL distribution installed"]
async fn a_wsl_session_leaves_the_distributions_own_files_alone() {
    if !wsl_is_available() {
        println!("skipped: this machine has no WSL distribution");
        return;
    }

    const HASH: &str = "md5sum ~/.bashrc ~/.profile | md5sum";
    let untouched = outside_the_session(HASH);
    let session = RealSession::wsl();

    let command = session.submit(HASH);
    let output = session.wsl_until(command, &untouched).await;

    assert!(
        output.contains(&untouched),
        "the startup files are the ones that were there before Acter started: expected \
         {untouched:?} in {output:?}"
    );
    assert_eq!(
        outside_the_session(HASH),
        untouched,
        "and they are still those after the session has run"
    );
}

/// The same question put to a `wsl.exe` Acter is not driving, so the comparison is against
/// the distribution as it stands rather than against another Acter session.
fn outside_the_session(command: &str) -> String {
    let answered = std::process::Command::new(WSL)
        .args(["--", "bash", "-c", command])
        .output()
        .expect("wsl answers");
    String::from_utf8_lossy(&answered.stdout)
        .split_whitespace()
        .next()
        .expect("md5sum prints a hash")
        .to_owned()
}

/// **The window attaches after the shell has already spoken, which is what actually
/// happens** — the session starts when the process does and the frontend attaches when its
/// page has loaded, and a shell that draws its prompt quickly does it into a sink nobody is
/// holding yet.
///
/// Found by the user on 2026-08-25: the status bar sat on "connecting" forever and the
/// session's first prompt never appeared, because both are emitted once, early, and were
/// dropped. This attaches deliberately late and asserts that nothing said in the meantime
/// was lost — and that a command submitted *afterwards* is still read aloud, which is the
/// third symptom of the same report.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_frontend_that_attaches_late_is_told_everything_it_missed() {
    let adapter = acter_shells::adapter_for(POWERSHELL);
    let launch = adapter.launch();
    let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
    let environment: Vec<(&str, &str)> = launch
        .environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let pty = LocalPty::spawn(&launch.program, &args, &environment, COLUMNS, SCREEN_LINES)
        .expect("a shell starts");
    let session = SessionService::start(
        Box::new(pty),
        Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
        Arc::new(RealClock::new()) as Arc<dyn Clock>,
        PacingConfig::default(),
        ShellFacts::of(adapter.as_ref()),
    );

    // The shell gets a head start: this is the webview loading.
    tokio::time::sleep(Duration::from_secs(5)).await;

    let events = Arc::new(Recorder::default());
    session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);
    let session = RealSession { session, events };

    let said = |session: &RealSession| -> Vec<SessionEvent> {
        session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .iter()
            .cloned()
            .collect()
    };

    assert!(
        said(&session).iter().any(|event| matches!(
            event,
            SessionEvent::ConnectionChanged {
                state: ConnectionState::Connected
            }
        )),
        "the window is told the session is usable, however late it asked: {:?}",
        said(&session)
    );
    assert!(
        said(&session)
            .iter()
            .any(|event| matches!(event, SessionEvent::PromptDrawn { .. })),
        "and the prompt drawn before it attached is not lost: {:?}",
        said(&session)
    );

    // The third symptom: a command run after all this is still read aloud.
    let command = session.submit("echo acter-after-a-late-attach");
    session
        .until(command, "acter-after-a-late-attach", PATIENCE)
        .await;
    tokio::time::sleep(SETTLE).await;

    assert!(
        said(&session).iter().any(|event| matches!(
            event,
            SessionEvent::Announce {
                announcement: Announcement::ReadAloud { text },
                ..
            } if text.contains("acter-after-a-late-attach")
        )),
        "output is still announced after a late attach: {:?}",
        said(&session)
    );
}

/// **B5.6 against a real shell**: the prompt PowerShell drew is reported, so a listener can
/// hear the working directory and the branch they are in.
///
/// The regression this pins was invisible to every earlier test in this file, because none
/// of them asked what a session says *between* blocks — and that is exactly where a prompt
/// lives once a shell marks all four boundaries.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_powershell_reports_the_prompt_it_drew() {
    let session = RealSession::powershell();

    let command = session.submit("echo acter-before-the-prompt");
    session
        .until(command, "acter-before-the-prompt", PATIENCE)
        .await;
    tokio::time::sleep(SETTLE).await;

    let prompts: Vec<String> = session
        .events
        .0
        .lock()
        .expect("recorder poisoned")
        .iter()
        .filter_map(|event| match event {
            SessionEvent::PromptDrawn { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !prompts.is_empty(),
        "a marked session says what its prompt says, and it said: {prompts:?}"
    );
    assert!(
        prompts.iter().any(|drawn| drawn.contains("PS ")),
        "and what it says is PowerShell's own prompt: {prompts:?}"
    );
}

/// **The whole of B5.2 against the shell it is about**: a real Windows PowerShell whose
/// injected snippet marks all four boundaries, so a block opens on `C`, closes on `D`, and
/// closes because the shell said the command ended rather than because a prompt reappeared.
///
/// This is the first test in this repository that can assert any of it. cmd can mark its
/// prompt and its command line and nothing else, so until now every block in every real
/// session ended by inference and no session had ever seen an exit code (spec B4.5).
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_real_powershell_marks_every_boundary_and_finishes_its_blocks() {
    let session = RealSession::powershell();

    let command = session.submit("echo acter-marked-line");
    let output = session.until(command, "acter-marked-line", PATIENCE).await;

    assert_eq!(
        session.heading_of(command),
        Some(Some("echo acter-marked-line".to_owned())),
        "the block is named by what the far end echoed between B and C"
    );
    assert!(
        output.contains("acter-marked-line"),
        "and holds the command's output: {output:?}"
    );
    assert!(
        !output.contains("echo acter-marked-line"),
        "and never the line the user typed, which belongs in the heading: {output:?}"
    );
    assert!(
        session.finished(command),
        "the block closed because the shell said so: {:?}",
        session.events.0.lock().expect("recorder poisoned")
    );
    assert!(
        !session
            .events
            .0
            .lock()
            .expect("recorder poisoned")
            .contains(&SessionEvent::IntegrationUnavailable),
        "a shell that marks all four boundaries is integrated, not flagged"
    );
}

/// **The line the user typed is never read back at them, in any block** — B4.9's
/// requirement against the first shell whose `B..C` region is the shell's own rather than
/// one the tracker synthesized from an echo.
///
/// Two commands, because this failure showed up on the second when it showed up at all: a
/// session whose first block is clean and whose every block after it glues the user's own
/// typing onto the answer.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn no_command_in_a_powershell_session_reads_the_typed_line_back() {
    let session = RealSession::powershell();

    let first = session.submit("echo acter-alpha");
    session.until(first, "acter-alpha", PATIENCE).await;

    let second = session.submit("echo acter-bravo");
    session.until(second, "acter-bravo", PATIENCE).await;

    let said = session.rendered();
    assert!(
        !said.contains("echo acter-bravo") && !said.contains("echo acter-alpha"),
        "no command line is read back at the user, in any block: {said:?}"
    );
    assert_eq!(
        session.heading_of(second),
        Some(Some("echo acter-bravo".to_owned())),
        "the command line belongs in the heading and nowhere else"
    );
}

/// **A line that invokes no command at all still gets its output into the block**, which is
/// the edge the output marker had to survive.
///
/// `C` comes from PowerShell's command-lookup hook and `1..3` looks nothing up, so had the
/// hook not fired those three lines would have landed in the `B..C` region — which
/// `Pump::wants` excludes, leaving the user with silence. Measured on both editions:
/// PowerShell resolves its formatting pipeline as a command, so the marker arrives before
/// the first line of output either way. A silent line is the worst failure this product
/// has, which is why an expression with no command in it earns a test of its own.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn a_line_that_invokes_no_command_still_says_what_it_produced() {
    let session = RealSession::powershell();

    let command = session.submit("1..3");
    let output = session.until(command, "3", PATIENCE).await;

    for expected in ["1", "2", "3"] {
        assert!(
            output.contains(expected),
            "every line of output reached the block: {expected} missing from {output:?}"
        );
    }
    assert!(session.finished(command), "and the block closed");
}

/// **Ctrl+D ends a real PowerShell session**, and the read channel closing is what ending
/// means — the only ending the transport port models (spec B4, decision 3).
///
/// What it costs in bytes is the adapter's answer and was measured rather than assumed:
/// neither `0x1a` nor `0x04` ends a PowerShell session on a pseudoconsole, so taking the
/// spec's expectation on trust would have shipped a keystroke that quietly did nothing
/// (spec B5.2, decision 5, amended).
///
/// The session is asserted to be *gone* rather than merely quiet: a submission after it
/// must not run. The frontend does not forward Ctrl+D yet, so this drives `send_key`
/// directly — the same call the router makes, and the same path a keyboard will take.
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn ctrl_d_ends_a_real_powershell_session() {
    let session = RealSession::powershell();

    let first = session.submit("echo acter-before-the-end");
    session.until(first, "acter-before-the-end", PATIENCE).await;

    assert_eq!(
        session.ctrl_d(),
        KeyAck::Applied,
        "PowerShell has an end-of-input answer and it went out"
    );

    // The shell echoes and runs whatever that answer was, so the ending is audible before
    // it is complete; what says it completed is that nothing runs afterwards.
    tokio::time::sleep(Duration::from_secs(3)).await;
    session.submit("echo acter-after-the-end");
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(
        !session.rendered().contains("acter-after-the-end"),
        "the session had ended, so nothing ran in it: {:?}",
        session.rendered()
    );
}

/// The same keystroke in a session over a shell nobody has measured writes nothing and says
/// so, rather than guessing at a byte.
///
/// `cmd.exe` stands in for that here only because it is the other shell this suite can
/// start; what is under test is the honest answer, not cmd (spec B5.2, decision 5).
#[tokio::test]
#[ignore = "spawns a real shell"]
async fn ctrl_d_in_a_shell_with_no_measured_answer_does_nothing() {
    let session = RealSession::marked();

    assert_eq!(session.ctrl_d(), KeyAck::NothingToActOn);

    let after = session.submit("echo acter-still-here");
    let output = session.until(after, "acter-still-here", PATIENCE).await;
    assert!(
        output.contains("acter-still-here"),
        "and the session is untouched: {output:?}"
    );
}

/// **Replacing a session really ends the shell it replaced** (spec B7, decision 4).
///
/// This is the acceptance criterion with teeth, and the reason it is a test rather than an
/// intention: dropping the outgoing `Arc<dyn SessionApi>` has to reach `LocalPty::drop`,
/// which is what kills the process. If any task, controller or router held a clone that
/// outlived it, the shell would survive invisibly, and a user who connected five times
/// would have five shells.
///
/// **The oracle is a file the shell holds open exclusively.** Counting processes by name is
/// no good on a developer's machine, and `LocalPty` exposes no process id — but a shell that
/// is alive is holding a `FileShare::None` handle, and a shell that is gone is not. So the
/// question "is that far end still running" is asked of the operating system directly, and
/// answers without ambiguity.
///
/// Everything above the factory is the shipped code: the real `ConnectService` doing the
/// real replace. What is a double here is only the factory, which starts a shell that locks
/// the file the profile names — because a profile that means "hold this file" is what makes
/// the far end observable at all.
mod replacing_a_session {
    use std::fs::OpenOptions;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    use acter_core::{ConnectApi, ConnectService, ProfileId, SessionFactory};

    use super::*;

    /// Long enough for PowerShell to start twice on a cold machine.
    const LOCK_PATIENCE: Duration = Duration::from_secs(30);

    static NONCE: AtomicU32 = AtomicU32::new(0);

    /// A path nothing else in this run will use.
    fn marker() -> PathBuf {
        let unique = NONCE.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("acter-b7-{}-{unique}.lock", std::process::id()))
    }

    /// Whether some process is holding this file open exclusively — which for these
    /// markers means the shell that created it is alive.
    fn held(path: &Path) -> bool {
        path.exists() && OpenOptions::new().write(true).open(path).is_err()
    }

    async fn until(patience: Duration, what: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            if ready() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {what}");
    }

    /// The one double: a factory whose sessions are real PowerShell processes, each holding
    /// open the file its profile named.
    struct LockingShells;

    impl SessionFactory for LockingShells {
        fn open(&self, profile: &ProfileId) -> Result<Arc<dyn SessionApi>, String> {
            let ProfileId::Program { program: path } = profile else {
                return Err("this factory only starts marker shells.".to_owned());
            };
            // `None` sharing is what makes the handle observable from outside: while this
            // process lives, nothing else can open the file for writing.
            let snippet = format!(
                "$global:acter = [System.IO.File]::Open('{path}', 'Create', 'Write', 'None')"
            );
            let launch = ShellLaunch {
                program: POWERSHELL.to_owned(),
                args: vec![
                    "-NoProfile".to_owned(),
                    "-NoExit".to_owned(),
                    "-Command".to_owned(),
                    snippet,
                ],
                environment: Vec::new(),
            };
            let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
            let pty = LocalPty::spawn(&launch.program, &args, &[], COLUMNS, SCREEN_LINES)?;
            Ok(Arc::new(SessionService::start(
                Box::new(pty),
                Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
                Arc::new(RealClock::new()) as Arc<dyn Clock>,
                PacingConfig::default(),
                ShellFacts {
                    markers: ShellMarkers::Full,
                    eof: None,
                },
            )))
        }
    }

    #[tokio::test]
    #[ignore = "spawns two real shells"]
    async fn connecting_twice_leaves_exactly_one_shell_running() {
        let first = marker();
        let second = marker();
        let service = ConnectService::new(
            Arc::new(LockingShells),
            Arc::new(ThisMachine::new()),
            Vec::new(),
        );

        service
            .use_profile(&ProfileId::Program {
                program: first.display().to_string(),
            })
            .expect("the first shell starts");
        until(LOCK_PATIENCE, "the first shell to take its lock", || {
            held(&first)
        })
        .await;

        service
            .use_profile(&ProfileId::Program {
                program: second.display().to_string(),
            })
            .expect("the second shell starts");
        until(LOCK_PATIENCE, "the second shell to take its lock", || {
            held(&second)
        })
        .await;

        // The replaced shell is gone: nothing is holding its file any more. This is the
        // assertion that fails if a clone of the outgoing session outlives the replace.
        until(LOCK_PATIENCE, "the replaced shell to exit", || {
            !held(&first)
        })
        .await;
        assert!(
            held(&second),
            "and the one the user is now on is still running"
        );

        drop(service);
        until(
            LOCK_PATIENCE,
            "the last shell to exit with the window",
            || !held(&second),
        )
        .await;

        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }
}
