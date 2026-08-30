//! The full pipeline, transcript in and recorded events out: a `ScriptedTransport`
//! feeding the real `SessionService`, which owns the real `AlacrittyEngine`, the real
//! `BoundaryTracker`, the real pacing policy and the real `SessionActor`, with a
//! recording `EventSink` on the far end.
//!
//! Nothing above the transport is faked. That is the entry's whole point: A3's fake
//! implemented `SessionApi` at the top of the stack and *scripted every verdict*, so no
//! manual session had ever exercised the policy, the actor, the tracker or the engine.
//! Here the verdicts are computed — `TooBig`, the patience announcement, the exit code —
//! from real text and real timing, and the transcript only supplies bytes.
//!
//! **Nothing sleeps.** Every scripted delay and every pacing deadline is B1.5's fake
//! clock, driven forward one armed deadline at a time, so a session that takes twelve
//! seconds of scripted time finishes in microseconds of real time.
//!
//! **The glue is gone, and that is what B6 delivered.** This file used to carry fifty
//! lines of scaffolding — command ids minted at `BlockStarted`, every region but `Output`
//! dropped, `take_replies` drained and written back — which B3.5 deliberately kept dumb
//! and named as B6's to promote (spec B3.5, decision 8). All of it is now
//! `SessionService`, so what is left here is a driver: a clock to advance, lines to
//! submit, keys to press, and the events that came back. The same transcripts and very
//! nearly the same assertions, against a real component instead of test scaffolding,
//! which is the strongest regression net the promotion could have had.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acter_core::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, Key, KeyAck, KeyPress,
    PacingConfig, SessionApi, SessionEvent, SessionId, SessionService, ShellFacts, ShellMarkers,
    SubmitAck, Timer, Transport, TransportError,
};
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, ScriptedTransport, SessionTranscript, TranscriptShell, Unmarked,
};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::task::yield_now;

/// The emulated screen. Eighty by twenty-four, so the built-in transcript's floods
/// genuinely scroll — which is how the extractor's scrolled-out path gets exercised
/// rather than assumed.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

/// The one session every test drives.
const SESSION: SessionId = SessionId(1);

/// The grace period these tests run under. Two hundred milliseconds rather than the
/// shipped five seconds, so a session that is going to be flagged is flagged inside the
/// scripted time each case already runs for. That the default is five seconds is
/// `PacingConfig`'s own test to pin; what is under test here is what a flagged session
/// then does.
const GRACE: Duration = Duration::from_millis(200);

/// Time moves only when a test says so — B1.5's fake clock, shared by the transport's
/// scripted delays, the service's grace period and the actor's pacing deadlines, so all
/// three are ordered against each other exactly as they would be against a real one.
#[derive(Default)]
struct FakeClock {
    now: Mutex<Duration>,
    armed: Mutex<Vec<(Duration, oneshot::Sender<()>)>>,
}

impl FakeClock {
    fn set_now(&self, at: Duration) {
        *self.now.lock().expect("clock poisoned") = at;
    }

    fn advance_to(&self, at: Duration) {
        self.set_now(at);
        let mut armed = self.armed.lock().expect("timers poisoned");
        let (due, pending) = std::mem::take(&mut *armed)
            .into_iter()
            .partition::<Vec<_>, _>(|(deadline, _)| *deadline <= at);
        *armed = pending;
        for (_, fire) in due {
            let _ = fire.send(());
        }
    }

    fn next_deadline(&self) -> Option<Duration> {
        self.armed
            .lock()
            .expect("timers poisoned")
            .iter()
            .map(|(deadline, _)| *deadline)
            .min()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        *self.now.lock().expect("clock poisoned")
    }

    fn timer(&self, after: Duration) -> Timer {
        let (fire, fired) = oneshot::channel();
        let deadline = self.now() + after;
        self.armed
            .lock()
            .expect("timers poisoned")
            .push((deadline, fire));
        Timer::new(fired)
    }
}

/// The far end of the protocol: everything the frontend would have received.
#[derive(Default)]
struct Recorder(Mutex<Vec<SessionEvent>>);

impl EventSink for Recorder {
    fn send(&self, event: SessionEvent) {
        self.0.lock().expect("recorder poisoned").push(event);
    }
}

impl Recorder {
    fn events(&self) -> Vec<SessionEvent> {
        self.0.lock().expect("recorder poisoned").clone()
    }

    fn len(&self) -> usize {
        self.0.lock().expect("recorder poisoned").len()
    }
}

/// What a test can still see of the transport after the service took ownership of it.
///
/// The service owns its transport by construction — `Transport` is `Send` and not `Sync`
/// with `&mut self` on every method — so a test that wants to know what was written to
/// the far end has to arrange it on the way in. This is that arrangement, and nothing
/// more: a decorator that copies what passes through.
#[derive(Default)]
struct FarEnd {
    written: Mutex<Vec<u8>>,
    /// A clone of the channel the transport delivers reads on, so the driver below can
    /// tell "the session has gone quiet" from "the pump has not caught up yet".
    reads: Mutex<Option<Sender<Vec<u8>>>>,
}

impl FarEnd {
    fn written(&self) -> Vec<u8> {
        self.written.lock().expect("far end poisoned").clone()
    }

    /// Reads produced but not yet consumed by the service.
    fn unread(&self) -> usize {
        match &*self.reads.lock().expect("far end poisoned") {
            Some(reads) => reads.max_capacity() - reads.capacity(),
            None => 0,
        }
    }
}

struct Recording {
    inner: ScriptedTransport,
    far_end: Arc<FarEnd>,
}

impl Transport for Recording {
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        *self.far_end.reads.lock().expect("far end poisoned") = Some(bytes.clone());
        self.inner.start(bytes);
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.far_end
            .written
            .lock()
            .expect("far end poisoned")
            .extend_from_slice(bytes);
        self.inner.write(bytes)
    }

    fn interrupt(&mut self) -> Result<(), TransportError> {
        self.inner.interrupt()
    }

    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        self.inner.resize(columns, screen_lines)
    }
}

/// One whole session, and the handle a test drives it by.
struct Pipeline {
    clock: Arc<FakeClock>,
    events: Arc<Recorder>,
    far_end: Arc<FarEnd>,
    session: SessionService,
}

impl Pipeline {
    /// The ordinary case: a transcript-backed far end, one delivery per read.
    fn start(transcript: SessionTranscript) -> Self {
        Self::over(Box::new(TranscriptShell::new(transcript)), Chunking::Whole)
    }

    /// Any far end, cut any way — which is what B3.6 bought: the fixtures below are
    /// replayed unchanged over a shell that emits no markers, and over a pipe that hands
    /// the engine one byte at a time.
    fn over(shell: Box<dyn FakeShell>, chunking: Chunking) -> Self {
        Self::marked(shell, chunking, ShellMarkers::Full)
    }

    /// The same, for a far end whose prompt marks only `A` and `B` — `cmd.exe` (spec
    /// B4.5). The declaration is the shell's, so it is named beside the shell.
    fn marked(shell: Box<dyn FakeShell>, chunking: Chunking, markers: ShellMarkers) -> Self {
        let clock = Arc::new(FakeClock::default());
        let events = Arc::new(Recorder::default());
        let far_end = Arc::new(FarEnd::default());
        let transport = Recording {
            inner: ScriptedTransport::with_shell(shell, chunking, clock.clone()),
            far_end: Arc::clone(&far_end),
        };
        let session = SessionService::start(
            Box::new(transport),
            Box::new(AlacrittyEngine::new(COLUMNS, SCREEN_LINES)),
            Arc::clone(&clock) as Arc<dyn Clock>,
            PacingConfig {
                integration_grace: GRACE,
                ..PacingConfig::default()
            },
            ShellFacts {
                markers,
                eof: None,
                setup: None,
                discards_line: None,
            },
        );
        session.attach_session(SESSION, Arc::clone(&events) as Arc<dyn EventSink>);

        Self {
            clock,
            events,
            far_end,
            session,
        }
    }

    /// Submits a line the way the frontend's edit field would.
    fn submit(&mut self, line: &str) -> CommandId {
        // The one place in these suites that reads an ack apart: a running session always
        // accepts, and the other answer belongs to a window with no session at all.
        match self.session.submit_command(SESSION, line) {
            SubmitAck::Accepted { command_id } => command_id,
            SubmitAck::NotConnected => panic!("a running session accepts a line"),
        }
    }

    /// Presses Ctrl+C the way the frontend will once A3.2 lands: the keystroke, not the
    /// meaning, and not the byte.
    fn press_ctrl_c(&mut self) -> KeyAck {
        self.session.send_key(
            SESSION,
            KeyPress {
                key: Key::Char('c'),
                ctrl: true,
                shift: false,
                alt: false,
            },
        )
    }

    /// Runs the session forward to `at`, one deadline at a time — whichever of the
    /// transport's scripted waits, the grace period, the rendering tick and the pacing
    /// window comes first.
    ///
    /// One at a time because every side arms its next deadline only once the current one
    /// has fired: jumping straight to `at` would deliver one step of a repeating sequence
    /// and leave every announcement it should have provoked in the future.
    async fn run_until(&mut self, at: u64) {
        let target = Duration::from_millis(at);
        loop {
            self.settle().await;
            let Some(next) = self.clock.next_deadline().filter(|next| *next <= target) else {
                break;
            };
            self.clock.advance_to(next.max(self.clock.now()));
            self.settle().await;
        }
        self.clock.set_now(target);
    }

    /// Yields until the session has nothing left to do at the current instant: every read
    /// the far end produced has been consumed, and no more events are coming out.
    ///
    /// The tasks are all in memory and only ever run because this test advanced the clock
    /// or submitted something, so quiescence really is quiescence rather than a guess at
    /// a delay — and the bound turns a wedged session into a legible failure instead of a
    /// hung suite.
    async fn settle(&mut self) {
        let mut quiet = 0;
        let mut seen = self.events.len();
        for _ in 0..100_000 {
            yield_now().await;
            let events = self.events.len();
            if events == seen && self.far_end.unread() == 0 {
                quiet += 1;
                if quiet == 16 {
                    return;
                }
            } else {
                quiet = 0;
                seen = events;
            }
        }
        panic!("the session never went quiet; saw {:?}", self.events());
    }

    fn events(&self) -> Vec<SessionEvent> {
        self.events.events()
    }

    /// Just the text the buffer would have shown, in order.
    fn rendered(&self) -> String {
        self.events()
            .iter()
            .filter_map(|event| match event {
                SessionEvent::Output { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// What a session said, with everything about *how* its bytes were cut left out.
    ///
    /// The comparison the byte-at-a-time suite is built on, and the reason it is stated
    /// this way rather than as event equality: see [`Substance`].
    fn substance(&self) -> Substance {
        let mut blocks: Vec<Block> = Vec::new();
        let find = |blocks: &mut Vec<Block>, command_id: CommandId, what: &str| -> usize {
            blocks
                .iter()
                .position(|block: &Block| block.command_id == command_id)
                .unwrap_or_else(|| {
                    panic!("{what} for a command that never started: {command_id:?}")
                })
        };
        for event in self.events() {
            match event {
                SessionEvent::CommandStarted {
                    command_id,
                    command_line,
                } => blocks.push(Block {
                    command_id,
                    command_line,
                    output: String::new(),
                    closed: false,
                }),
                SessionEvent::Output {
                    command_id, text, ..
                } => {
                    let at = find(&mut blocks, command_id, "output");
                    blocks[at].output.push_str(&text);
                }
                SessionEvent::CommandFinished { command_id } => {
                    let at = find(&mut blocks, command_id, "a command finished that");
                    blocks[at].closed = true;
                }
                _ => {}
            }
        }
        Substance {
            unintegrated: self.unintegrated(),
            blocks,
            announcements: self.announcements(),
        }
    }

    /// Whether this session was flagged as having no shell integration. The observable
    /// form of what the glue used to keep as a boolean of its own: the service owns the
    /// integration state now, and this is what it says about it out loud.
    fn unintegrated(&self) -> bool {
        self.events()
            .contains(&SessionEvent::IntegrationUnavailable)
    }

    fn announcements(&self) -> Vec<Announcement> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                SessionEvent::Announce { announcement, .. } => Some(announcement),
                _ => None,
            })
            .collect()
    }
}

/// What a session said, independent of how its bytes arrived.
///
/// **Not the event stream, and deliberately not.** Replaying a fixture one byte at a
/// time does not produce an identical stream: the engine emits a line item per advance,
/// so a byte-at-a-time read produces many more `Appended` revisions than one whole read
/// does. That is correct behavior rather than a defect, so claiming event equality would
/// be claiming something false. What must be identical is this — the concatenated output
/// text per command, the block structure, the exit codes, whether the session was flagged
/// unintegrated, and what was said out loud. That is B2's cardinal property (text is
/// never lost) plus B3's marker recognition, asserted across the whole fixture suite
/// (spec B3.6, decision 3).
#[derive(Debug, PartialEq, Eq)]
struct Substance {
    unintegrated: bool,
    blocks: Vec<Block>,
    announcements: Vec<Announcement>,
}

/// One command block: what it was called, everything it said, and whether it ended.
///
/// It recorded the exit code until A6 took that off `CommandFinished`. A success has no
/// code on the wire at all now, so `Some(ExitCode(0))` is not a thing a block can
/// observe; what these tests were really distinguishing is a block the markers closed
/// from one that never closed, which is what `closed` says. A failure's code is still
/// checked, on the `Announcement::Failed` that carries it.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    command_id: CommandId,
    /// The heading the frontend would put on this block: what the far end echoed, read
    /// out of the byte stream by the real engine and tracker (spec B6.1, decision 1).
    command_line: Option<String>,
    output: String,
    closed: bool,
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

fn loaded(name: &str) -> SessionTranscript {
    SessionTranscript::load(fixture(name)).unwrap_or_else(|why| panic!("{name}: {why}"))
}

/// Every test submits exactly one command, so the id the service mints is always the
/// first. The command line is the one the far end echoed back — every caller here submits
/// a line a shell reads, so it is the line that was submitted (spec B6.1, decision 1).
fn started(command_line: &str) -> SessionEvent {
    SessionEvent::CommandStarted {
        command_id: CommandId(1),
        command_line: Some(command_line.to_owned()),
    }
}

fn output(text: &str) -> SessionEvent {
    SessionEvent::Output {
        command_id: CommandId(1),
        text: text.to_owned(),
    }
}

/// Takes no exit code since A6: closing a block and reporting a failure are two events,
/// and the code rides `Announcement::Failed`.
fn finished() -> SessionEvent {
    SessionEvent::CommandFinished {
        command_id: CommandId(1),
    }
}

fn announce(announcement: Announcement) -> SessionEvent {
    SessionEvent::Announce {
        command_id: CommandId(1),
        announcement,
    }
}

/// The moment the far end first spoke, which is what A9 calls connected — not the moment a
/// process was spawned, because a shell that has not said anything is not one anybody can
/// use. It leads every session, so it leads every expectation here.
fn connected() -> SessionEvent {
    SessionEvent::ConnectionChanged {
        state: ConnectionState::Connected,
    }
}

/// The prompt the transcript draws, as the frontend now hears it (spec B5.6). Before that
/// entry a marked session could not say what its prompt said at all: the `A..B` region is
/// excluded from block content, so the working directory and the branch a listener steers by
/// were audible nowhere.
fn prompt(text: &str) -> SessionEvent {
    SessionEvent::PromptDrawn {
        text: text.to_owned(),
    }
}

/// The happy path, and the first time DESIGN's echo exclusion has been tested against
/// something that actually echoes: the prompt the shell drew and the command line it
/// echoed back are both in the byte stream, and neither reaches the frontend as output.
#[tokio::test]
async fn a_command_produces_its_output_and_nothing_the_shell_said_around_it() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;
    // Nothing has been read yet, so nothing has been said. The prompt the transcript draws
    // arrives with the first read and is asserted below, where it now leads the sequence:
    // drawing a prompt is not a command, but since B5.6 it is not silence either.
    // The first read is the far end saying hello, which is A9's definition of connected;
    // nothing else has happened yet.
    assert_eq!(pipeline.events(), vec![connected()]);

    pipeline.submit("small");
    pipeline.run_until(1_000).await;

    assert_eq!(
        pipeline.events(),
        vec![
            connected(),
            prompt("acter>"),
            started("small"),
            output("hello from acter"),
            // The last word on the output comes before the event that ends the command:
            // it describes text that arrived while the command was running (spec A3.2).
            announce(Announcement::ReadAloud {
                text: "hello from acter".to_owned()
            }),
            finished(),
        ]
    );
    assert!(
        !pipeline.unintegrated(),
        "the markers arrived, so the grace period passed without a word"
    );
}

/// The verdict is no longer scripted: thirty lines of real text measured by the real
/// policy against `PacingConfig`'s real threshold.
#[tokio::test]
async fn a_flood_is_announced_by_size_because_the_policy_measured_it() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.submit("big");
    pipeline.run_until(1_000).await;

    assert_eq!(
        pipeline.announcements(),
        vec![Announcement::TooBig { lines: 30 }],
        "counted from the text, not read off the fixture"
    );
    assert!(pipeline.rendered().starts_with("line 1\nline 2\n"));
    assert!(
        pipeline.rendered().ends_with("line 30"),
        "every line reaches the buffer even when none of it is read aloud"
    );
    assert!(pipeline.events().contains(&finished()));
}

#[tokio::test]
async fn a_failing_command_carries_its_exit_code_out_of_the_marker() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.submit("fail");
    pipeline.run_until(1_000).await;

    assert_eq!(
        pipeline.events(),
        vec![
            connected(),
            prompt("acter>"),
            started("fail"),
            output("error: the command reported a problem"),
            // The error text, then the ending, then the verdict about it. A6 decision 2
            // put `Failed` after the output it judges; A3.2 put the last word on that
            // output before the ending, for the same reason.
            announce(Announcement::ReadAloud {
                text: "error: the command reported a problem".to_owned()
            }),
            finished(),
            announce(Announcement::Failed {
                exit_code: ExitCode(2)
            }),
        ]
    );
}

/// A full-screen program: the switch travels in the item stream, so the repaint it writes
/// in the same read is attributed to the alternate screen rather than to the command the
/// user just ran.
#[tokio::test]
async fn entering_the_alternate_screen_reaches_the_actor_in_stream_order() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.submit("nano");
    pipeline.run_until(10_000).await;

    let events = pipeline.events();
    let position = |wanted: &SessionEvent| {
        events
            .iter()
            .position(|event| event == wanted)
            .unwrap_or_else(|| panic!("expected {wanted:?} in {events:?}"))
    };

    // The session's own prompt comes first now (spec B5.6), and what this test is about
    // begins after it: the command, then the screen switch, in the order they were read.
    assert_eq!(events.first(), Some(&connected()));
    assert_eq!(events.get(1), Some(&prompt("acter>")));
    assert_eq!(events.get(2), Some(&started("nano")));
    assert_eq!(
        events.get(3),
        Some(&SessionEvent::AltScreenEntered),
        "the switch is placed before the repaint that shared its read"
    );
    assert!(position(&SessionEvent::AltScreenLeft) < position(&finished()));
    assert!(
        pipeline.rendered().contains("GNU nano 7.2")
            && pipeline.rendered().contains("editing a file"),
        "every row the program painted is rendered, including the one above the cursor          it painted over: {:?}",
        pipeline.rendered()
    );
}

/// An interrupting *line*: a far end that treats some submitted line as an interrupt,
/// the way A3.1's `stop` did before A3.2 gave the user a real key and retired it from the
/// shipped transcript. It cancels the sequence in flight and closes the block with a
/// marker carrying no exit code at all, and because nobody asked the transport to
/// interrupt, that block ended: the service says finished, not stopped.
///
/// The far end is a fixture rather than the built-in shell, because this is the pipe's
/// behavior and not a scenario the product ships — but it is behavior a real shell can
/// still produce, so it keeps its test.
#[tokio::test]
async fn an_interrupting_line_ends_the_running_command_without_inventing_a_failure() {
    let mut pipeline = Pipeline::start(loaded("interrupting_line.json"));
    pipeline.run_until(0).await;

    pipeline.submit("hang");
    pipeline.run_until(3_000).await;
    assert!(pipeline.events().contains(&started("hang")));

    pipeline.submit("halt");
    pipeline.run_until(4_000).await;

    let events = pipeline.events();
    assert!(events.contains(&finished()), "the block closed: {events:?}");
    assert!(
        !pipeline
            .announcements()
            .iter()
            .any(|announcement| matches!(announcement, Announcement::Failed { .. })),
        "a command the user stopped did not fail"
    );
}

/// The user-facing interrupt, end to end for the first time: a keystroke the frontend
/// reports, the keybinding policy, `Transport::interrupt`, the far end's own interrupt
/// rule, a `D` with no exit code — and, because the service knows it asked, an event that
/// says stopped rather than "finished, exit code 0" (spec B6, decision 8).
#[tokio::test]
async fn pressing_ctrl_c_stops_the_running_command_and_says_so() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.submit("forever");
    pipeline.run_until(3_000).await;
    assert!(pipeline.events().contains(&started("forever")));

    assert_eq!(pipeline.press_ctrl_c(), KeyAck::Applied);
    pipeline.run_until(4_000).await;

    let events = pipeline.events();
    assert!(
        events.contains(&SessionEvent::CommandInterrupted {
            command_id: CommandId(1)
        }),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::CommandFinished { .. })),
        "a stopped command is never also reported as finished: {events:?}"
    );
    assert_eq!(
        pipeline.press_ctrl_c(),
        KeyAck::NothingToActOn,
        "and with the command over there is nothing left to stop"
    );
}

/// DESIGN's reliability model, one test per case. First: a marker cut in half by a read
/// boundary, which is the case an event-level fake structurally cannot produce.
///
/// It used to be one hand-authored fixture that split one marker. It is now a property
/// of the pipe, so *every* marker in this session arrives in pieces — the A, the B, the
/// C and the D, each cut into seven reads — and the session still says exactly what it
/// says when each arrives whole (spec B3.6, decision 3).
#[tokio::test]
async fn a_marker_split_across_two_reads_is_still_one_marker() {
    let mut pipeline = Pipeline::over(Box::new(TranscriptShell::builtin()), Chunking::Bytes(1));
    pipeline.run_until(0).await;

    pipeline.submit("small");
    pipeline.run_until(1_000).await;

    assert!(
        !pipeline.unintegrated(),
        "the markers were recognized in pieces"
    );
    assert_eq!(
        pipeline.substance().blocks,
        vec![Block {
            command_id: CommandId(1),
            command_line: Some("small".to_owned()),
            output: "hello from acter".to_owned(),
            closed: true,
        }],
        "one block, opened and closed by markers nobody ever received whole, headed by an          echo delivered one byte at a time"
    );
}

/// A program forging a command-end marker inside its own output. The block closes where
/// the forgery said it did — a shell's prompt cannot reappear before its command ended,
/// so believing the marker is the only reading available — and everything after it is
/// unstructured text rather than a second command's output.
#[tokio::test]
async fn a_forged_command_end_closes_the_block_and_what_follows_is_not_output() {
    let mut pipeline = Pipeline::start(loaded("forged_marker.json"));
    pipeline.run_until(0).await;

    pipeline.submit("forge");
    pipeline.run_until(2_000).await;

    let events = pipeline.events();
    assert_eq!(
        events.iter().filter(|event| **event == finished()).count(),
        1,
        "the real marker that followed found no open block: {events:?}"
    );
    assert!(pipeline.rendered().contains("before the forgery"));
    assert!(
        !pipeline.rendered().contains("after the forgery"),
        "text outside a block is rendered by the frontend, never as this command's output"
    );
}

/// A command whose end marker never arrives. The patience announcement comes from the
/// real policy after the real ten-second window, measured on the fake clock, and the
/// command is never reported as finished.
#[tokio::test]
async fn a_command_that_never_ends_announces_that_it_is_still_running() {
    let mut pipeline = Pipeline::start(loaded("no_end_marker.json"));
    pipeline.run_until(0).await;

    pipeline.submit("hang");
    pipeline.run_until(9_000).await;
    assert!(
        pipeline.announcements().is_empty(),
        "nothing is said before the window closes: {:?}",
        pipeline.announcements()
    );

    pipeline.run_until(11_000).await;

    assert_eq!(pipeline.announcements(), vec![Announcement::StillRunning]);
    assert!(
        !pipeline
            .events()
            .iter()
            .any(|event| matches!(event, SessionEvent::CommandFinished { .. })),
        "nothing ended, so nothing may claim it did"
    );
    assert!(
        pipeline.rendered().contains("still working"),
        "the buffer keeps up even while speech stays quiet"
    );
}

/// DESIGN's reliability case 2, which is **Decided** and until B6 happened nowhere: a
/// shell with no integration at all is flagged, announced, and every command in it
/// degrades to case 1 — patience announcement, manual buffer review, no auto-read.
///
/// This test used to assert the opposite. It asserted that an unmarked session produces
/// *zero* events, and its message called that correct: "with no block, there is no
/// command to report". That was true of the glue and is not shippable as the default
/// backend — it is a silent terminal with an empty buffer — so the service opens a
/// command at submission and closes it at the next one (spec B6, decision 10).
///
/// It is the *built-in* shell with its markers taken away, not a transcript that answers
/// only the handful of lines somebody thought to write down: integration missing is
/// something that happens to a working shell (spec B3.6, decision 4).
#[tokio::test]
async fn a_session_with_no_markers_degrades_honestly_instead_of_going_silent() {
    let mut pipeline = Pipeline::over(
        Box::new(Unmarked::new(TranscriptShell::builtin())),
        Chunking::Whole,
    );

    pipeline.run_until(500).await;
    // **This assertion used to be the whole sequence, and that is what 27.4 was** (spec
    // B6.2). The far end had drawn `acter>` before any of this, and the pump discarded it
    // for having fallen in no region — so the listener met a connected session with an
    // empty buffer, and a flag explaining an emptiness that had nothing to do with
    // integration. The prompt now arrives first, in a block nobody submitted, and is read
    // aloud; the flag follows when the grace period expires, which is what it has always
    // meant.
    assert_eq!(
        pipeline.events(),
        vec![
            // Connected first, because the far end spoke: an unmarked session is still a
            // session, and A9's state is about the transport rather than about
            // integration.
            connected(),
            SessionEvent::CommandStarted {
                command_id: CommandId(1),
                command_line: None,
            },
            SessionEvent::Output {
                command_id: CommandId(1),
                text: "acter>".to_owned(),
            },
            // Then the flag, which is what this test is here for.
            SessionEvent::IntegrationUnavailable,
            SessionEvent::Announce {
                command_id: CommandId(1),
                announcement: Announcement::ReadAloud {
                    text: "acter>".to_owned(),
                },
            },
        ],
        "the session says what the far end said, and then what happened to it"
    );

    pipeline.submit("forever");
    pipeline.run_until(14_000).await;

    // The flag is still the first thing said *about the session's integration*; what comes
    // before it is the transport reporting that the far end spoke at all (A9), and the
    // prompt that far end had already drawn.
    assert_eq!(pipeline.events().first(), Some(&connected()));
    assert_eq!(
        pipeline
            .events()
            .iter()
            .position(|event| *event == SessionEvent::IntegrationUnavailable),
        Some(3),
        "after the connection and the prompt, and before anything the user did"
    );
    assert!(
        pipeline.events().contains(&SessionEvent::CommandStarted {
            // The *second* id, because the prompt this shell drew before anybody typed
            // anything spent the first one (spec B6.2). A block nobody submitted is a
            // real block with a real id, which is what `Pump::unclaimed` has always
            // meant; what changed is that an unintegrated session now has one at all.
            command_id: CommandId(2),
            // **The far end's echo is the boundary now** (spec B4.4), and it is also the
            // heading: there is no `B..C` region without markers, but the echo is still
            // on the wire, and the row it was written onto is what opens this block.
            command_line: Some("forever".to_owned()),
        }),
        "the echo is the boundary, so the command the user typed exists: {:?}",
        pipeline.events()
    );
    assert!(
        pipeline.rendered().contains("still working"),
        "and its output reaches the buffer, which is the whole of manual review: {:?}",
        pipeline.rendered()
    );
    assert!(
        !pipeline.rendered().contains("forever"),
        "**and the echoed command line is not in the buffer** (spec B4.4). It used to be,          because echo exclusion is DESIGN's `B..C` rule and there is no such region          without markers. The echo is what opens the block now, and it becomes the          heading rather than the first line under it: {:?}",
        pipeline.rendered()
    );
    assert!(
        pipeline
            .announcements()
            .contains(&Announcement::StillRunning),
        "patience still fires — that is what degrading to case 1 means: {:?}",
        pipeline.announcements()
    );
    // **And it is read aloud, which B4.4 inverted.** Degrading to case 1 still means no
    // exit code and no verdict, and the patience announcement above is still what tells a
    // listener the command is going; what it no longer means is silence. Repeating text
    // that genuinely arrived is evidence, not the guess the old rule was avoiding.
    assert!(
        pipeline.announcements().iter().any(|announcement| matches!(
            announcement,
            Announcement::ReadAloud { text } if text.contains("phase one")
        )),
        "and its output is read aloud: {:?}",
        pipeline.announcements()
    );
}

/// The loop `TerminalEngine::take_replies` was built for, closed for the first time: a
/// program asks where the cursor is, the engine formats the answer, and the service
/// writes it back to the transport, which records it. Drop it and the program waits
/// forever, which for this product is a session that has simply gone quiet.
#[tokio::test]
async fn a_device_query_is_answered_back_to_the_transport() {
    let mut pipeline = Pipeline::start(loaded("device_query.json"));
    pipeline.run_until(0).await;

    pipeline.submit("where");
    pipeline.run_until(1_000).await;

    let written = String::from_utf8_lossy(&pipeline.far_end.written()).into_owned();
    // A carriage return is what the domain sends for Enter, and what a real shell acts
    // on: a bare line feed is echoed and never run (spec B4).
    let answer = written
        .strip_prefix("where\r")
        .expect("the submitted line comes first");
    assert!(
        answer.starts_with('\x1b') && answer.ends_with('R'),
        "a cursor position report was written back, got {answer:?}"
    );
}

/// A `cmd.exe`-shaped session end to end over the real engine and the real tracker: the
/// prompt carries `A` and `B`, nothing carries `C` or `D`, and the block structure has to
/// come from the echo (spec B4.5, decisions 2 to 4).
///
/// This is the whole of 22.5 in one assertion. Before it, setting the environment variable
/// did not degrade such a session, it deleted it — no `CommandStarted`, no output, nothing
/// spoken — because `BlockStarted` only ever came from a `C` and `wants` accepted only
/// `Output`.
#[tokio::test]
async fn a_shell_that_marks_only_its_prompt_still_gets_blocks_and_speaks() {
    let mut pipeline = Pipeline::marked(
        far_end("cmd_prompt.json"),
        Chunking::Whole,
        ShellMarkers::PromptAndCommandLine,
    );
    pipeline.run_until(0).await;

    pipeline.submit("dir");
    pipeline.run_until(1_000).await;
    pipeline.submit("quiet");
    pipeline.run_until(2_000).await;

    let substance = pipeline.substance();
    assert!(
        !substance.unintegrated,
        "a shell that marks A and B is integrated: it is one shell's shortcoming, not a \
         third integration state (ROADMAP 22.5, pinned)"
    );

    let commands: Vec<_> = substance
        .blocks
        .iter()
        .filter(|block| block.command_line.is_some())
        .collect();
    assert_eq!(
        commands.len(),
        2,
        "one block per submitted command, and it is the synthesized C that opens each: {:?}",
        substance.blocks
    );

    let dir = commands[0];
    assert_eq!(dir.command_line.as_deref(), Some("dir"));
    assert!(
        dir.output.contains("one.txt") && dir.output.contains("two.txt"),
        "the command's output is under its own heading: {:?}",
        dir.output
    );
    assert!(
        !dir.output.contains("dir\r") && !dir.output.starts_with("dir"),
        "and the echo of the command line is not, which is DESIGN's echo exclusion doing \
         what the markers were injected to let it do: {:?}",
        dir.output
    );
    assert!(
        dir.output.contains("acter>"),
        "the returning prompt is the last thing the block says — the only ending a shell \
         with no exit code has to offer (spec B4.5, decision 4): {:?}",
        dir.output
    );
    assert!(dir.closed, "and the block closes");

    let quiet = commands[1];
    assert_eq!(
        quiet.command_line.as_deref(),
        Some("quiet"),
        "a command that printed nothing still opens a block and names it: {:?}",
        substance.blocks
    );
    assert!(
        quiet.output.contains("acter>"),
        "with the returning prompt as its content: {:?}",
        quiet.output
    );
}

/// One replayable session: a far end, how long to let it settle before anything is typed,
/// and the lines submitted to it with the scripted moment each is given to answer by.
struct Case {
    name: &'static str,
    far_end: &'static str,
    /// Scripted time to run before the first submission. Zero for a shell that announces
    /// itself, because its markers arrive with its first prompt; past the grace period
    /// for one that does not, so the session has resolved before the user types — which
    /// is also the order a person experiences, since the announcement comes first.
    warmup: u64,
    submissions: &'static [(&'static str, u64)],
}

impl Case {
    async fn replay(&self, chunking: Chunking) -> Substance {
        let mut pipeline = Pipeline::marked(far_end(self.far_end), chunking, markers(self.far_end));
        pipeline.run_until(self.warmup).await;
        for (line, until) in self.submissions {
            pipeline.submit(line);
            pipeline.run_until(*until).await;
        }
        pipeline.substance()
    }
}

/// A far end by name: the built-in shell, the built-in shell with its integration taken
/// away, or a transcript fixture from disk.
/// What the far end of that name is able to mark, which is a fact about the shell rather
/// than about the session — the container derives it the same way, from which shell was
/// named (spec B4.5, decision 1).
fn markers(name: &str) -> ShellMarkers {
    match name {
        "cmd_prompt.json" => ShellMarkers::PromptAndCommandLine,
        _ => ShellMarkers::Full,
    }
}

fn far_end(name: &str) -> Box<dyn FakeShell> {
    match name {
        "builtin" => Box::new(TranscriptShell::builtin()),
        "unmarked builtin" => Box::new(Unmarked::new(TranscriptShell::builtin())),
        fixture => Box::new(TranscriptShell::new(loaded(fixture))),
    }
}

/// Every session this crate can replay: the ten built-in scenarios and every fixture,
/// each with enough scripted time to finish saying what it has to say.
const CASES: &[Case] = &[
    Case {
        name: "a small answer",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("small", 1_000)],
    },
    Case {
        name: "a flood",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("big", 1_000)],
    },
    Case {
        name: "a failure with an exit code",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("fail", 1_000)],
    },
    Case {
        name: "three phases with quiescent gaps",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("slow", 6_000)],
    },
    Case {
        name: "an announcement long enough to be read by size",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("speech", 1_000)],
    },
    Case {
        name: "a sampled trickle",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("tail", 100_000)],
    },
    Case {
        name: "a flood then a trickle",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("burst", 60_000)],
    },
    Case {
        name: "a full-screen program",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("nano", 10_000)],
    },
    Case {
        name: "a command interrupted while it runs",
        far_end: "interrupting_line.json",
        warmup: 0,
        submissions: &[("hang", 3_000), ("halt", 4_000)],
    },
    Case {
        name: "an unrecognized line",
        far_end: "builtin",
        warmup: 0,
        submissions: &[("nothing scripted this", 1_000)],
    },
    Case {
        name: "a far end with no integration",
        far_end: "unmarked builtin",
        warmup: 500,
        submissions: &[("small", 2_000)],
    },
    Case {
        name: "alt_screen.json",
        far_end: "alt_screen.json",
        warmup: 0,
        submissions: &[("nano", 1_000)],
    },
    Case {
        name: "captured_prompt.json",
        far_end: "captured_prompt.json",
        warmup: 0,
        submissions: &[("hello", 1_000)],
    },
    // `cmd.exe`'s shape: `A`, the prompt, `B` and nothing else, with the echo of the
    // submitted line written onto the prompt's own row. Two commands, one with output and
    // one without, because a command that prints nothing is where a synthesized `C` has to
    // come from the next prompt's `A` rather than from a new row (spec B4.5, decision 2).
    Case {
        name: "cmd_prompt.json",
        far_end: "cmd_prompt.json",
        warmup: 0,
        submissions: &[("dir", 1_000), ("quiet", 2_000)],
    },
    // **A real PowerShell session, recorded** (spec B5.2, decision 6). The startup payload
    // is a capture off a real pseudoconsole rather than something anyone wrote, and it is
    // the first fixture here whose markers are terminated the way a real shell terminates
    // them — `ESC \`, not the `BEL` every hand-authored fixture reaches for. What it buys
    // is that everything above the transport can be tested against real PowerShell
    // behaviour without PowerShell.
    //
    // **What is recorded is the prompt the shell drew, and the host's startup chatter in
    // front of it is not.** The capture opened with a cursor-position query, mode sets, a
    // title and Windows PowerShell's own notice that it has turned PSReadLine off in front
    // of a screen reader — followed by an absolute cursor move to the row below all that.
    // A transcript far end is not a console: the query has nobody to answer it here
    // (`device_query.json` is where that path is tested deliberately), and the cursor move
    // lands the fake shell's echo on top of a row the capture had already written, which
    // makes what a block contains depend on where the reads were cut. None of that is the
    // shell talking, so none of it is what this fixture is for.
    //
    // Three commands, because the shape differs: one with output, one that prints nothing
    // and ends nonzero, and one whose rule nothing matches.
    Case {
        name: "powershell_prompt.json",
        far_end: "powershell_prompt.json",
        warmup: 0,
        submissions: &[
            ("echo acter-hello", 1_000),
            ("cmd /c exit 3", 2_000),
            ("nothing scripted this", 3_000),
        ],
    },
    Case {
        name: "device_query.json",
        far_end: "device_query.json",
        warmup: 0,
        submissions: &[("where", 1_000)],
    },
    Case {
        name: "forged_marker.json",
        far_end: "forged_marker.json",
        warmup: 0,
        submissions: &[("forge", 2_000)],
    },
    Case {
        name: "no_end_marker.json",
        far_end: "no_end_marker.json",
        warmup: 0,
        submissions: &[("hang", 11_000)],
    },
];

/// **The headline test of B3.6**, and the reason the entry exists.
///
/// DESIGN's "a marker split across two reads" used to be proven by one hand-authored
/// fixture that split one marker. Read boundaries are a property of the pipe rather than
/// of what the far end said, so they became [`Chunking`] — and the case became a
/// dimension every session is replayed under. Under `Bytes(1)` every marker, every
/// escape sequence, every device query and every line of output arrives one byte at a
/// time.
///
/// What it asserts is [`Substance`], not the event stream: byte-at-a-time reads produce
/// many more `Appended` revisions, which is correct, so event equality would be a false
/// claim. Text, blocks, exit codes, integration and speech are what must not move.
#[tokio::test]
async fn every_session_says_the_same_thing_when_every_byte_is_its_own_read() {
    for case in CASES {
        let whole = case.replay(Chunking::Whole).await;
        let byte_at_a_time = case.replay(Chunking::Bytes(1)).await;

        assert_eq!(
            byte_at_a_time, whole,
            "{} said something different when its bytes were cut differently",
            case.name
        );
    }
}

/// The other half of the same claim, and the one that keeps the test above from passing
/// vacuously: every case really does drive a session through the whole pipeline.
///
/// A block with nothing in it counts — the default rule opens and closes one without
/// saying a word, and that block *is* the substance being compared. The one exception
/// states itself: a far end with no integration is flagged rather than trusted, and its
/// one block is the degraded one the submission opened, which never closes structurally
/// and so has no exit code to report (spec B6, decision 10).
#[tokio::test]
async fn every_case_in_the_suite_actually_produces_a_session() {
    for case in CASES {
        let substance = case.replay(Chunking::Bytes(1)).await;

        if case.far_end == "unmarked builtin" {
            assert!(substance.unintegrated, "{}: {substance:?}", case.name);
            // Two, since B4.4: the shell's own prompt arrives before anything has been
            // submitted, so it belongs to no command and gets a block of its own, and the
            // command's block opens when the far end echoes it.
            assert_eq!(substance.blocks.len(), 2, "{}: {substance:?}", case.name);
            assert!(
                !substance.blocks[1].output.is_empty(),
                "{}: a degraded session still puts its text in the buffer: {substance:?}",
                case.name
            );
            assert!(
                !substance.blocks[1].closed,
                "{}: and a command that never ends structurally never closes",
                case.name
            );
            continue;
        }

        assert!(
            !substance.unintegrated,
            "{} never recognized a marker: {substance:?}",
            case.name
        );
        assert!(
            !substance.blocks.is_empty(),
            "{} opened no command block: {substance:?}",
            case.name
        );
    }
}

/// The companion to the case above, and the one B6's manual NVDA pass sent me looking
/// for: a command that *completes* in an unintegrated session, rather than one that runs
/// forever.
///
/// The two are not the same path. `forever` never reaches a quiescence flush with a
/// finished command behind it, so it exercises the patience announcement and the babble
/// guard; a command that ends exercises the flush of the remainder, which is the only
/// place `ReadAloud` is produced. Without this, the suppression that DESIGN's reliability
/// case 2 requires was asserted only where it was never really tested.
///
/// With no markers there is no `D` either, so the command stays open until the next
/// submission closes it — which is exactly decision 10's model, and why the assertion
/// here is about what was said rather than about the exit code.
#[tokio::test]
async fn a_completed_command_in_an_unintegrated_session_is_read_aloud() {
    let mut pipeline = Pipeline::over(
        Box::new(Unmarked::new(TranscriptShell::builtin())),
        Chunking::Whole,
    );

    pipeline.run_until(500).await;
    pipeline.submit("small");
    pipeline.run_until(3_000).await;

    assert!(
        pipeline.rendered().contains("hello from acter"),
        "the output still reaches the buffer, which is the whole of manual review: {:?}",
        pipeline.rendered()
    );
    assert!(
        !pipeline.rendered().contains("small"),
        "**and the echoed command line is not in the buffer** (spec B4.4): it is the          heading, not the block's first content line: {:?}",
        pipeline.rendered()
    );
    // **Inverted by B4.4.** This asserted that neither the echo nor the output was read
    // aloud, which was DESIGN's reliability case 2 before B4.4 amended it. A session that
    // renders everything and speaks none of it is silent to the only user this product
    // has, and repeating text that genuinely arrived is evidence rather than the guess the
    // no-auto-read rule was written to avoid.
    assert!(
        pipeline.announcements().iter().any(|announcement| matches!(
            announcement,
            Announcement::ReadAloud { text } if text.contains("hello from acter")
        )),
        "the output of a session with no integration is read aloud: {:?}",
        pipeline.announcements()
    );
}

/// B4.2, at the layer where it actually happens: the real `AlacrittyEngine` scrolling a
/// real grid, rather than `TerminalItem`s handed to a fake.
///
/// The service-level tests pin the pump's rule. They cannot pin the premise that rule
/// rests on, which is that the extractor really does re-emit a *previous* command's rows
/// as `Settled` while the next one is running. Nothing ever tells it a boundary happened
/// — `place` calls `settle_block` only on a `D` marker, and an unmarked shell sends none
/// — so its on-screen rows keep their ids and their text across the submission. Every
/// new row then pushes one of them above the view, and it arrives complete in whichever
/// block is open.
///
/// `big` is thirty lines on a twenty-four line screen, so the rows still live when it
/// finishes are `big`'s. `small` is one line plus a prompt and an echo: enough scrolling
/// to drag several of them over the boundary, one per new row, which is the signature the
/// user's capture showed.
#[tokio::test]
async fn a_finished_commands_rows_do_not_scroll_into_the_next_block() {
    let mut pipeline = Pipeline::over(
        Box::new(Unmarked::new(TranscriptShell::builtin())),
        Chunking::Whole,
    );

    pipeline.run_until(500).await;
    pipeline.submit("big");
    pipeline.run_until(3_000).await;
    pipeline.submit("small");
    pipeline.run_until(6_000).await;

    let substance = pipeline.substance();
    // Three blocks, since B4.4: the shell's prompt arrives before anything is submitted
    // and belongs to no command, then each submission's block opens when the far end
    // echoes it.
    let [_prompt, flood, after] = &substance.blocks[..] else {
        panic!("a prompt block and two submissions: {substance:?}");
    };

    let flooded: Vec<&str> = flood.output.lines().map(str::trim_end).collect();
    for row in 1..=30 {
        let expected = format!("line {row}");
        assert!(
            flooded.contains(&expected.as_str()),
            "the flood's own block keeps every row it produced, and lost {expected:?}: \
             {flooded:?}"
        );
    }
    assert!(
        after.output.contains("hello from acter"),
        "the second block still has its own output: {:?}",
        after.output
    );
    assert!(
        !after.output.contains("line "),
        "and none of the first command's, however much the screen scrolled under it: {:?}",
        after.output
    );
}
