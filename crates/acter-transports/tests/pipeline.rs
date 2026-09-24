//! The full pipeline, transcript in and recorded events out: a `ScriptedTransport` feeding
//! the real `SessionService` and `AlacrittyEngine`, with a recording `EventSink` on the far end.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acter_core::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, Key, KeyAck, KeyPress,
    LineId, LineRevision, PacingConfig, SessionApi, SessionEvent, SessionId, SessionService,
    ShellFacts, ShellMarkers, SubmitAck, Timer, Transport, TransportError,
};
use acter_term::AlacrittyEngine;
use acter_transports::{
    Chunking, FakeShell, ScriptedTransport, SessionTranscript, TranscriptShell, Unmarked,
};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::task::yield_now;

const COLUMNS: u16 = 80;
/// Fewer rows than the built-in `big` prints, so a flood scrolls.
const SCREEN_LINES: u16 = 24;

const SESSION: SessionId = SessionId(1);

/// Must stay under the 500 ms `warmup` of the unmarked cases, so they are flagged before
/// the first submission.
const GRACE: Duration = Duration::from_millis(200);

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

#[derive(Default)]
struct FarEnd {
    written: Mutex<Vec<u8>>,
    reads: Mutex<Option<Sender<Vec<u8>>>>,
}

impl FarEnd {
    fn written(&self) -> Vec<u8> {
        self.written.lock().expect("far end poisoned").clone()
    }

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

struct Pipeline {
    clock: Arc<FakeClock>,
    events: Arc<Recorder>,
    far_end: Arc<FarEnd>,
    session: SessionService,
}

impl Pipeline {
    fn start(transcript: SessionTranscript) -> Self {
        Self::over(Box::new(TranscriptShell::new(transcript)), Chunking::Whole)
    }

    fn over(shell: Box<dyn FakeShell>, chunking: Chunking) -> Self {
        Self::marked(shell, chunking, ShellMarkers::Full)
    }

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

    fn submit(&mut self, line: &str) -> CommandId {
        match self.session.submit_command(SESSION, line) {
            SubmitAck::Accepted { command_id } => command_id,
            SubmitAck::NotConnected => panic!("a running session accepts a line"),
        }
    }

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

    /// One deadline at a time, because each side arms its next deadline only after the
    /// current one has fired.
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

    /// Renumbers output line ids from zero in order of first appearance, because the engine
    /// also mints ids for rows no event ever shows.
    fn events(&self) -> Vec<SessionEvent> {
        let mut seen: Vec<LineId> = Vec::new();
        self.events
            .events()
            .into_iter()
            .map(|event| match event {
                SessionEvent::Output {
                    command_id,
                    line,
                    revision,
                    text,
                } => {
                    let at = seen
                        .iter()
                        .position(|known| *known == line)
                        .unwrap_or_else(|| {
                            seen.push(line);
                            seen.len() - 1
                        });
                    SessionEvent::Output {
                        command_id,
                        line: LineId(at as u64),
                        revision,
                        text,
                    }
                }
                other => other,
            })
            .collect()
    }

    fn rendered(&self) -> String {
        let mut lines: Vec<(LineId, String)> = Vec::new();
        for event in self.events() {
            let SessionEvent::Output {
                line,
                revision,
                text,
                ..
            } = event
            else {
                continue;
            };
            match lines.iter_mut().find(|(known, _)| *known == line) {
                Some((_, held)) if revision == LineRevision::Appended => held.push_str(&text),
                Some((_, held)) => *held = text,
                None => lines.push((line, text)),
            }
        }
        lines
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

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
                    lines: Vec::new(),
                    closed: false,
                }),
                SessionEvent::Output {
                    command_id,
                    line,
                    revision,
                    text,
                } => {
                    let at = find(&mut blocks, command_id, "output");
                    blocks[at].apply(line, revision, &text);
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

/// A byte-at-a-time replay emits more `Appended` revisions than a whole read, so replays
/// are compared on this rather than on the event stream.
#[derive(Debug, PartialEq, Eq)]
struct Substance {
    unintegrated: bool,
    blocks: Vec<Block>,
    announcements: Vec<Announcement>,
}

#[derive(Debug, PartialEq, Eq)]
struct Block {
    command_id: CommandId,
    command_line: Option<String>,
    lines: Vec<(LineId, String)>,
    closed: bool,
}

impl Block {
    fn apply(&mut self, line: LineId, revision: LineRevision, text: &str) {
        match self.lines.iter_mut().find(|(known, _)| *known == line) {
            Some((_, held)) if revision == LineRevision::Appended => held.push_str(text),
            Some((_, held)) => *held = text.to_owned(),
            None => self.lines.push((line, text.to_owned())),
        }
    }

    fn output(&self) -> String {
        self.lines
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

fn loaded(name: &str) -> SessionTranscript {
    SessionTranscript::load(fixture(name)).unwrap_or_else(|why| panic!("{name}: {why}"))
}

fn started(command_line: &str) -> SessionEvent {
    SessionEvent::CommandStarted {
        command_id: CommandId(1),
        command_line: Some(command_line.to_owned()),
    }
}

fn output(text: &str) -> SessionEvent {
    output_on(0, LineRevision::Appended, text)
}

fn output_on(line: u64, revision: LineRevision, text: &str) -> SessionEvent {
    SessionEvent::Output {
        command_id: CommandId(1),
        line: LineId(line),
        revision,
        text: text.to_owned(),
    }
}

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

fn connected() -> SessionEvent {
    SessionEvent::ConnectionChanged {
        state: ConnectionState::Connected,
    }
}

fn prompt(text: &str) -> SessionEvent {
    SessionEvent::PromptDrawn {
        text: text.to_owned(),
    }
}

#[tokio::test]
async fn a_command_produces_its_output_and_nothing_the_shell_said_around_it() {
    let mut pipeline = Pipeline::start(SessionTranscript::builtin());
    pipeline.run_until(0).await;
    assert_eq!(
        pipeline.events(),
        vec![connected(), prompt("acter>")],
        "the first prompt is reported when it is drawn, before anything is typed at it"
    );

    pipeline.submit("small");
    pipeline.run_until(1_000).await;

    assert_eq!(
        pipeline.events(),
        vec![
            connected(),
            prompt("acter>"),
            started("small"),
            output("hello from acter"),
            announce(Announcement::ReadAloud {
                text: "hello from acter".to_owned()
            }),
            finished(),
            prompt("acter>"),
        ]
    );
    assert!(
        !pipeline.unintegrated(),
        "the markers arrived, so the grace period passed without a word"
    );
}

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
            announce(Announcement::ReadAloud {
                text: "error: the command reported a problem".to_owned()
            }),
            finished(),
            announce(Announcement::Failed {
                exit_code: ExitCode(2)
            }),
            prompt("acter>"),
        ]
    );
}

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
            lines: vec![(LineId(0), "hello from acter".to_owned())],
            closed: true,
        }],
        "one block, opened and closed by markers nobody ever received whole, headed by an          echo delivered one byte at a time"
    );
}

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

#[tokio::test]
async fn a_session_with_no_markers_degrades_honestly_instead_of_going_silent() {
    let mut pipeline = Pipeline::over(
        Box::new(Unmarked::new(TranscriptShell::builtin())),
        Chunking::Whole,
    );

    pipeline.run_until(500).await;
    assert_eq!(
        pipeline.events(),
        vec![
            connected(),
            SessionEvent::CommandStarted {
                command_id: CommandId(1),
                command_line: None,
            },
            output_on(0, LineRevision::Appended, "acter>"),
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
            command_id: CommandId(2),
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
    assert!(
        pipeline.announcements().iter().any(|announcement| matches!(
            announcement,
            Announcement::ReadAloud { text } if text.contains("phase one")
        )),
        "and its output is read aloud: {:?}",
        pipeline.announcements()
    );
}

#[tokio::test]
async fn a_device_query_is_answered_back_to_the_transport() {
    let mut pipeline = Pipeline::start(loaded("device_query.json"));
    pipeline.run_until(0).await;

    pipeline.submit("where");
    pipeline.run_until(1_000).await;

    let written = String::from_utf8_lossy(&pipeline.far_end.written()).into_owned();
    let answer = written
        .strip_prefix("where\r")
        .expect("the submitted line comes first");
    assert!(
        answer.starts_with('\x1b') && answer.ends_with('R'),
        "a cursor position report was written back, got {answer:?}"
    );
}

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
        dir.output().contains("one.txt") && dir.output().contains("two.txt"),
        "the command's output is under its own heading: {:?}",
        dir.output()
    );
    assert!(
        !dir.output().contains("dir\r") && !dir.output().starts_with("dir"),
        "and the echo of the command line is not, which is DESIGN's echo exclusion doing \
         what the markers were injected to let it do: {:?}",
        dir.output()
    );
    assert!(
        dir.output().contains("acter>"),
        "the returning prompt is the last thing the block says — the only ending a shell \
         with no exit code has to offer (spec B4.5, decision 4): {:?}",
        dir.output()
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
        quiet.output().contains("acter>"),
        "with the returning prompt as its content: {:?}",
        quiet.output()
    );
}

struct Case {
    name: &'static str,
    far_end: &'static str,
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
    Case {
        name: "cmd_prompt.json",
        far_end: "cmd_prompt.json",
        warmup: 0,
        submissions: &[("dir", 1_000), ("quiet", 2_000)],
    },
    // Captured from Windows PowerShell on a real pseudoconsole, with the startup output before
    // the first prompt cut: its absolute cursor move lands the echo on an already-written row.
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

#[tokio::test]
async fn every_case_in_the_suite_actually_produces_a_session() {
    for case in CASES {
        let substance = case.replay(Chunking::Bytes(1)).await;

        if case.far_end == "unmarked builtin" {
            assert!(substance.unintegrated, "{}: {substance:?}", case.name);
            assert_eq!(substance.blocks.len(), 2, "{}: {substance:?}", case.name);
            assert!(
                !substance.blocks[1].output().is_empty(),
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
    assert!(
        pipeline.announcements().iter().any(|announcement| matches!(
            announcement,
            Announcement::ReadAloud { text } if text.contains("hello from acter")
        )),
        "the output of a session with no integration is read aloud: {:?}",
        pipeline.announcements()
    );
}

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
    let [_prompt, flood, after] = &substance.blocks[..] else {
        panic!("a prompt block and two submissions: {substance:?}");
    };

    let flood_output = flood.output();
    let flooded: Vec<&str> = flood_output.lines().map(str::trim_end).collect();
    for row in 1..=30 {
        let expected = format!("line {row}");
        assert!(
            flooded.contains(&expected.as_str()),
            "the flood's own block keeps every row it produced, and lost {expected:?}: \
             {flooded:?}"
        );
    }
    assert!(
        after.output().contains("hello from acter"),
        "the second block still has its own output: {:?}",
        after.output()
    );
    assert!(
        !after.output().contains("line "),
        "and none of the first command's, however much the screen scrolled under it: {:?}",
        after.output()
    );
}
