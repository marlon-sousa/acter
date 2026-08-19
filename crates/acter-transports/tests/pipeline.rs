//! The full pipeline, transcript in and recorded events out: a `ScriptedTransport`
//! feeding the real `AlacrittyEngine`, the real `BoundaryTracker`, the real pacing policy
//! and the real `SessionActor`, with a recording `EventSink` on the far end.
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
//! **The glue below is deliberately dumb, and B6 replaces it.** Command ids are minted in
//! submission order when a block opens, every region but `Output` is dropped (DESIGN's
//! echo exclusion, which B2 decision 2 promised would be a caller's one-line filter), and
//! `take_replies` is drained after each advance and written back to the transport. It is
//! not a component: no service, no controller, and nothing new in `acter-core`.
//! Correlation and the integration grace period need the queue of submitted commands,
//! which is B6's to own (spec B3.5, decision 8).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acter_core::{
    Announcement, BoundaryEvent, BoundaryTracker, Clock, CommandId, EventSink, ExitCode, LineId,
    LineRevision, PacingConfig, ReadMode, Region, Screen, SessionActor, SessionEvent, SessionInput,
    TerminalEngine, Timer, Transport, Wake,
};
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, ScriptedTransport, SessionTranscript, TranscriptShell, Unmarked,
};
use tokio::sync::mpsc::{Receiver, channel};
use tokio::sync::oneshot;
use tokio::task::yield_now;

/// The emulated screen. Eighty by twenty-four, so the built-in transcript's floods
/// genuinely scroll — which is how the extractor's scrolled-out path gets exercised
/// rather than assumed.
const COLUMNS: u16 = 80;
const SCREEN_LINES: u16 = 24;

/// Read buffering, large enough that no test observes back-pressure.
const READS: usize = 1024;

/// Time moves only when a test says so — B1.5's fake clock, shared by the transport's
/// scripted delays and the actor's pacing deadlines, so the two are ordered against each
/// other exactly as they would be against a real one.
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
}

/// One whole session: transport, engine, tracker, actor, and the glue between them.
struct Pipeline {
    clock: Arc<FakeClock>,
    transport: ScriptedTransport,
    reads: Receiver<Vec<u8>>,
    engine: AlacrittyEngine,
    tracker: BoundaryTracker,
    actor: SessionActor,
    events: Arc<Recorder>,
    /// B6's job, done here in one line: ids in submission order, minted where the block
    /// opens.
    next_id: u32,
    open: Option<CommandId>,
    /// Whether shell integration was ever observed. B6 turns this into the session state
    /// and its grace period; here it only says whether the tracker ever saw a marker.
    integrated: bool,
    /// What has been done with each line of the running command: `false` once some of its
    /// text has been forwarded, `true` if it was rewritten and its final text is still
    /// owed. Kept across regions on purpose — the engine settles a block's lines while
    /// the region is still `Output`, including the prompt row the echo was written onto,
    /// and it is knowing that row's id that keeps the echo out of the command's output.
    lines: HashMap<LineId, bool>,
    /// The last line whose text was forwarded, so consecutive lines are separated the way
    /// the pacing policy counts them.
    last_line: Option<LineId>,
    render_at: Option<Duration>,
    pacing_at: Option<Duration>,
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
        let clock = Arc::new(FakeClock::default());
        let events = Arc::new(Recorder::default());
        let mut transport = ScriptedTransport::with_shell(shell, chunking, clock.clone());
        let (sender, reads) = channel(READS);
        transport.start(sender);

        Self {
            actor: SessionActor::new(PacingConfig::default(), clock.clone(), events.clone()),
            clock,
            transport,
            reads,
            engine: AlacrittyEngine::new(COLUMNS, SCREEN_LINES),
            tracker: BoundaryTracker::new(),
            events,
            next_id: 0,
            open: None,
            integrated: false,
            lines: HashMap::new(),
            last_line: None,
            render_at: None,
            pacing_at: None,
        }
    }

    /// Submits a line the way the frontend's edit field would.
    fn submit(&mut self, line: &str) {
        self.transport
            .write(format!("{line}\n").as_bytes())
            .expect("the session is open");
    }

    /// One read, all the way through: bytes to items, items to boundary events, boundary
    /// events to the actor — and the engine's device-query answers back to the transport.
    fn feed(&mut self, bytes: &[u8]) {
        let items = self.engine.advance(bytes);
        for event in self.tracker.observe(items) {
            match event {
                BoundaryEvent::MarkersObserved => self.integrated = true,
                BoundaryEvent::BlockStarted => {
                    self.next_id += 1;
                    let command_id = CommandId(self.next_id);
                    self.open = Some(command_id);
                    self.last_line = None;
                    self.dispatch(SessionInput::CommandStarted { command_id });
                }
                BoundaryEvent::Line {
                    region,
                    id,
                    text,
                    revision,
                } => {
                    if let Some(text) = self.due(id, text, revision)
                        && region == Region::Output
                    {
                        self.output(id, text);
                    }
                }
                BoundaryEvent::BlockEnded { exit } => {
                    if let Some(command_id) = self.open.take() {
                        self.lines.clear();
                        self.dispatch(SessionInput::CommandEnded {
                            command_id,
                            // A marker with no usable code still ended the command. Which
                            // exit code an interrupted command deserves is B6's and A6's;
                            // stranding the session in "running" is the one answer that
                            // is certainly wrong.
                            exit_code: exit.unwrap_or(ExitCode(0)),
                        });
                    }
                }
                BoundaryEvent::ScreenChanged(Screen::Alternate) => {
                    self.dispatch(SessionInput::AltScreenEntered);
                }
                BoundaryEvent::ScreenChanged(Screen::Normal) => {
                    self.dispatch(SessionInput::AltScreenLeft);
                }
            }
        }

        // `take_replies`' first consumer: an emulator does not answer a device query
        // itself, and a program that asked one waits forever if nobody writes the answer
        // back.
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            self.transport.write(&replies).expect("the session is open");
        }
    }

    /// The text this item owes the speech path, if any: an append always, a settlement
    /// only when it is the line's first or last word, a rewrite never (DESIGN's separate
    /// paths — the buffer applies all three, speech does not).
    fn due(&mut self, id: LineId, text: String, revision: LineRevision) -> Option<String> {
        match revision {
            LineRevision::Appended => {
                self.lines.insert(id, false);
                Some(text)
            }
            LineRevision::Rewritten => {
                self.lines.insert(id, true);
                None
            }
            // Owed when the line was rewritten since its last word, and when it was never
            // seen at all — which is how a line that scrolled out of the screen area
            // inside a single read arrives, settled and complete, having never appended.
            LineRevision::Settled => self.lines.remove(&id).unwrap_or(true).then_some(text),
        }
    }

    /// Forwards one line's text as output, separated from the line before it. The
    /// separator is the glue's, not the engine's: a line item carries no line ending, and
    /// the pacing policy counts lines.
    fn output(&mut self, id: LineId, text: String) {
        let separated = match self.last_line {
            Some(last) if last == id => text,
            None => text,
            Some(_) => format!("\n{text}"),
        };
        self.last_line = Some(id);
        self.dispatch(SessionInput::Output { text: separated });
    }

    fn dispatch(&mut self, input: SessionInput) {
        self.actor.handle(input);
        self.reschedule();
    }

    fn reschedule(&mut self) {
        let requests = self.actor.take_requests();
        let now = self.clock.now();
        self.render_at = wake(self.render_at, requests.render, now);
        self.pacing_at = wake(self.pacing_at, requests.pacing, now);
    }

    /// Runs the session forward to `at`, one deadline at a time — whichever of the
    /// transport's scripted waits, the rendering tick and the pacing window comes first.
    ///
    /// One at a time because both sides arm their next deadline only once the current one
    /// has fired: jumping straight to `at` would deliver one step of a repeating sequence
    /// and leave every announcement it should have provoked in the future.
    async fn run_until(&mut self, at: u64) {
        let target = Duration::from_millis(at);
        loop {
            self.drain().await;
            let next = [self.clock.next_deadline(), self.render_at, self.pacing_at]
                .into_iter()
                .flatten()
                .min();
            let Some(next) = next.filter(|next| *next <= target) else {
                break;
            };

            self.clock.advance_to(next.max(self.clock.now()));
            // Bytes first, then the deadlines: output landing exactly on a pacing
            // window arrived before the window closed, which is what a real reader would
            // have seen too.
            self.drain().await;
            if self.render_at.is_some_and(|render| render <= next) {
                self.render_at = None;
                self.actor.wake_render();
                self.reschedule();
            }
            if self.pacing_at.is_some_and(|pacing| pacing <= next) {
                self.pacing_at = None;
                self.actor.wake_pacing();
                self.reschedule();
            }
        }
        self.clock.set_now(target);
    }

    /// Feeds everything the transport has produced, up to the point where its emission
    /// loop is parked on its next timer.
    async fn drain(&mut self) {
        let mut quiet = 0;
        while quiet < 2 {
            yield_now().await;
            let mut produced = false;
            while let Ok(chunk) = self.reads.try_recv() {
                self.feed(&chunk);
                produced = true;
            }
            quiet = if produced { 0 } else { quiet + 1 };
        }
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
        for event in self.events() {
            match event {
                SessionEvent::CommandStarted { command_id } => blocks.push(Block {
                    command_id,
                    output: String::new(),
                    exit_code: None,
                }),
                SessionEvent::Output {
                    command_id, text, ..
                } => match blocks
                    .iter_mut()
                    .find(|block| block.command_id == command_id)
                {
                    Some(block) => block.output.push_str(&text),
                    None => panic!("output for a command that never started: {command_id:?}"),
                },
                SessionEvent::CommandFinished {
                    command_id,
                    exit_code,
                    ..
                } => match blocks
                    .iter_mut()
                    .find(|block| block.command_id == command_id)
                {
                    Some(block) => block.exit_code = Some(exit_code),
                    None => panic!("a command finished that never started: {command_id:?}"),
                },
                _ => {}
            }
        }
        Substance {
            integrated: self.integrated,
            blocks,
            announcements: self.announcements(),
        }
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

fn wake(current: Option<Duration>, requested: Wake, now: Duration) -> Option<Duration> {
    match requested {
        Wake::Unchanged => current,
        Wake::Clear => None,
        Wake::After(after) => Some(now + after),
    }
}

/// What a session said, independent of how its bytes arrived.
///
/// **Not the event stream, and deliberately not.** Replaying a fixture one byte at a
/// time does not produce an identical stream: the engine emits a line item per advance,
/// so a byte-at-a-time read produces many more `Appended` revisions than one whole read
/// does. That is correct behavior rather than a defect, so claiming event equality would
/// be claiming something false. What must be identical is this — the concatenated output
/// text per command, the block structure, the exit codes, whether markers were
/// recognized at all, and what was said out loud. That is B2's cardinal property (text
/// is never lost) plus B3's marker recognition, asserted across the whole fixture suite
/// (spec B3.6, decision 3).
#[derive(Debug, PartialEq, Eq)]
struct Substance {
    integrated: bool,
    blocks: Vec<Block>,
    announcements: Vec<Announcement>,
}

/// One command block: what it was called, everything it said, and how it ended.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    command_id: CommandId,
    output: String,
    exit_code: Option<ExitCode>,
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

fn loaded(name: &str) -> SessionTranscript {
    SessionTranscript::load(fixture(name)).unwrap_or_else(|why| panic!("{name}: {why}"))
}

/// Every test submits exactly one command, so the id the glue mints is always the first.
fn started() -> SessionEvent {
    SessionEvent::CommandStarted {
        command_id: CommandId(1),
    }
}

fn output(text: &str) -> SessionEvent {
    SessionEvent::Output {
        command_id: CommandId(1),
        text: text.to_owned(),
        read_mode: ReadMode::Quiet,
    }
}

fn finished(exit_code: i32) -> SessionEvent {
    SessionEvent::CommandFinished {
        command_id: CommandId(1),
        exit_code: ExitCode(exit_code),
        read_mode: ReadMode::Quiet,
    }
}

fn announce(announcement: Announcement) -> SessionEvent {
    SessionEvent::Announce {
        command_id: CommandId(1),
        announcement,
    }
}

/// The happy path, and the first time DESIGN's echo exclusion has been tested against
/// something that actually echoes: the prompt the shell drew and the command line it
/// echoed back are both in the byte stream, and neither reaches the frontend as output.
#[tokio::test]
async fn a_command_produces_its_output_and_nothing_the_shell_said_around_it() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;
    assert!(
        pipeline.events().is_empty(),
        "drawing a prompt is not a command: {:?}",
        pipeline.events()
    );

    pipeline.submit("small");
    pipeline.run_until(1_000).await;

    assert_eq!(
        pipeline.events(),
        vec![
            started(),
            output("hello from acter"),
            finished(0),
            announce(Announcement::ReadAloud {
                text: "hello from acter".to_owned()
            }),
        ]
    );
    assert!(pipeline.integrated, "the markers were observed");
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
    assert!(pipeline.events().contains(&finished(0)));
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
            started(),
            output("error: the command reported a problem"),
            finished(2),
            announce(Announcement::ReadAloud {
                text: "error: the command reported a problem".to_owned()
            }),
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

    assert_eq!(events.first(), Some(&started()));
    assert_eq!(
        events.get(1),
        Some(&SessionEvent::AltScreenEntered),
        "the switch is placed before the repaint that shared its read"
    );
    assert!(position(&SessionEvent::AltScreenLeft) < position(&finished(0)));
    assert!(
        pipeline.rendered().contains("GNU nano 7.2")
            && pipeline.rendered().contains("editing a file"),
        "every row the program painted is rendered, including the one above the cursor          it painted over: {:?}",
        pipeline.rendered()
    );
}

/// An interrupt cancels the sequence in flight and closes the block with a marker that
/// carries no exit code at all.
#[tokio::test]
async fn an_interrupt_ends_the_running_command_without_inventing_a_failure() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.submit("forever");
    pipeline.run_until(3_000).await;
    assert!(pipeline.events().contains(&started()));

    pipeline.submit("stop");
    pipeline.run_until(4_000).await;

    let events = pipeline.events();
    assert!(
        events.contains(&finished(0)),
        "the block closed: {events:?}"
    );
    assert!(
        !pipeline
            .announcements()
            .iter()
            .any(|announcement| matches!(announcement, Announcement::Failed { .. })),
        "a command the user stopped did not fail"
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

    assert!(pipeline.integrated, "the markers were recognized in pieces");
    assert_eq!(
        pipeline.substance().blocks,
        vec![Block {
            command_id: CommandId(1),
            output: "hello from acter".to_owned(),
            exit_code: Some(ExitCode(0)),
        }],
        "one block, opened and closed by markers nobody ever received whole"
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
        events.iter().filter(|event| **event == finished(0)).count(),
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

/// A shell with no integration at all. Every line is unstructured, so no command block
/// ever opens and the session never claims to be integrated — DESIGN's reliability case
/// 2, which B6 turns into the grace period.
///
/// It is the *built-in* shell with its markers taken away, not a transcript that answers
/// only the handful of lines somebody thought to write down: integration missing is
/// something that happens to a working shell (spec B3.6, decision 4).
#[tokio::test]
async fn a_session_with_no_markers_never_reports_a_command_or_integration() {
    let mut pipeline = Pipeline::over(
        Box::new(Unmarked::new(TranscriptShell::builtin())),
        Chunking::Whole,
    );
    pipeline.run_until(0).await;

    pipeline.submit("small");
    pipeline.run_until(2_000).await;

    assert!(!pipeline.integrated, "no marker ever arrived");
    assert!(
        pipeline.events().is_empty(),
        "with no block, there is no command to report: {:?}",
        pipeline.events()
    );
}

/// The loop `TerminalEngine::take_replies` was built for, closed for the first time: a
/// program asks where the cursor is, the engine formats the answer, and the glue writes
/// it back to the transport, which records it. Drop it and the program waits forever,
/// which for this product is a session that has simply gone quiet.
#[tokio::test]
async fn a_device_query_is_answered_back_to_the_transport() {
    let mut pipeline = Pipeline::start(loaded("device_query.json"));
    pipeline.run_until(0).await;

    pipeline.submit("where");
    pipeline.run_until(1_000).await;

    let written = String::from_utf8_lossy(pipeline.transport.written()).into_owned();
    let answer = written
        .strip_prefix("where\n")
        .expect("the submitted line comes first");
    assert!(
        answer.starts_with('\x1b') && answer.ends_with('R'),
        "a cursor position report was written back, got {answer:?}"
    );
}

/// A resize is accepted by the transport and recorded, which is the whole observable
/// effect a scripted session can have: the grid it would reflow belongs to the engine.
#[tokio::test]
async fn a_resize_reaches_the_transport() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;

    pipeline.engine.resize(100, 30);
    pipeline
        .transport
        .resize(100, 30)
        .expect("the session is open");

    assert_eq!(pipeline.transport.last_resize(), Some((100, 30)));
}

/// One replayable session: a far end, and the lines submitted to it with the scripted
/// moment each is given to answer by.
struct Case {
    name: &'static str,
    far_end: &'static str,
    submissions: &'static [(&'static str, u64)],
}

impl Case {
    async fn replay(&self, chunking: Chunking) -> Substance {
        let mut pipeline = Pipeline::over(far_end(self.far_end), chunking);
        pipeline.run_until(0).await;
        for (line, until) in self.submissions {
            pipeline.submit(line);
            pipeline.run_until(*until).await;
        }
        pipeline.substance()
    }
}

/// A far end by name: the built-in shell, the built-in shell with its integration taken
/// away, or a transcript fixture from disk.
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
        submissions: &[("small", 1_000)],
    },
    Case {
        name: "a flood",
        far_end: "builtin",
        submissions: &[("big", 1_000)],
    },
    Case {
        name: "a failure with an exit code",
        far_end: "builtin",
        submissions: &[("fail", 1_000)],
    },
    Case {
        name: "three phases with quiescent gaps",
        far_end: "builtin",
        submissions: &[("slow", 6_000)],
    },
    Case {
        name: "an announcement long enough to be read by size",
        far_end: "builtin",
        submissions: &[("speech", 1_000)],
    },
    Case {
        name: "a sampled trickle",
        far_end: "builtin",
        submissions: &[("tail", 100_000)],
    },
    Case {
        name: "a flood then a trickle",
        far_end: "builtin",
        submissions: &[("burst", 60_000)],
    },
    Case {
        name: "a full-screen program",
        far_end: "builtin",
        submissions: &[("nano", 10_000)],
    },
    Case {
        name: "a command interrupted while it runs",
        far_end: "builtin",
        submissions: &[("forever", 3_000), ("stop", 4_000)],
    },
    Case {
        name: "an unrecognized line",
        far_end: "builtin",
        submissions: &[("nothing scripted this", 1_000)],
    },
    Case {
        name: "a far end with no integration",
        far_end: "unmarked builtin",
        submissions: &[("small", 2_000)],
    },
    Case {
        name: "alt_screen.json",
        far_end: "alt_screen.json",
        submissions: &[("nano", 1_000)],
    },
    Case {
        name: "captured_prompt.json",
        far_end: "captured_prompt.json",
        submissions: &[("hello", 1_000)],
    },
    Case {
        name: "device_query.json",
        far_end: "device_query.json",
        submissions: &[("where", 1_000)],
    },
    Case {
        name: "forged_marker.json",
        far_end: "forged_marker.json",
        submissions: &[("forge", 2_000)],
    },
    Case {
        name: "no_end_marker.json",
        far_end: "no_end_marker.json",
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
/// claim. Text, blocks, exit codes, marker recognition and speech are what must not move.
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
/// states itself: a far end with no integration produces no blocks at all, which is
/// DESIGN's reliability case 2 rather than a case that failed to run.
#[tokio::test]
async fn every_case_in_the_suite_actually_produces_a_session() {
    for case in CASES {
        let substance = case.replay(Chunking::Bytes(1)).await;

        if case.far_end == "unmarked builtin" {
            assert!(!substance.integrated, "{}: {substance:?}", case.name);
            assert!(substance.blocks.is_empty(), "{}: {substance:?}", case.name);
            continue;
        }

        assert!(
            substance.integrated,
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
