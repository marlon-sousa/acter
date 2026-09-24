//! Service: `SessionService` — one session owned end to end, as the actor task and a pump task.
//!
//! The pump is the transport's only owner, so a device-query answer can never overtake a
//! submitted line; `SessionApi` methods never wait on it, they `try_send` and return.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::select;
use tokio::spawn;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender, channel, unbounded_channel};

use crate::{
    Anchor, Binding, BoundaryEvent, BoundaryTracker, Caret, Clock, CommandId, ConnectionState,
    EventSink, ExitCode, FarEndAnswer, Integration, Key, KeyAck, KeyPress, Keystroke, LineId,
    LineOwner, LineRevision, PacingConfig, Region, RowChange, Screen, SessionActor, SessionApi,
    SessionEvent, SessionId, SessionInput, SessionIntent, ShellFacts, ShellMarkers, SubmitAck,
    TerminalEngine, Timer, Transport, binding_for, far_end_row, key_bytes,
};

const READS: usize = 1024;

const REQUESTS: usize = 64;

const BRACKET_START: &[u8] = b"\x1b[200~";
const BRACKET_END: &[u8] = b"\x1b[201~";

/// A carriage return, never a line feed; see `crates/acter-core/src/policies/key_bytes.rs`.
const ENTER: char = '\r';

pub struct SessionService {
    requests: Sender<Request>,
    next_id: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    far_end_line: Arc<AtomicBool>,
    eof: Option<Vec<u8>>,
    sink: Arc<AttachedSink>,
}

impl SessionService {
    /// Must be called from within a tokio runtime.
    pub fn start(
        mut transport: Box<dyn Transport>,
        engine: Box<dyn TerminalEngine + Send>,
        clock: Arc<dyn Clock>,
        config: PacingConfig,
        shell: ShellFacts,
    ) -> Self {
        let markers = shell.markers;
        let discards_line = shell.discards_line;
        let setup = shell.setup.map(|setup| setup.line);
        let sink = Arc::new(AttachedSink::default());
        let (bytes, reads) = channel(READS);
        transport.start(bytes);

        let (inputs, facts) = unbounded_channel();
        let actor = SessionActor::new(
            config,
            Arc::clone(&clock),
            Arc::clone(&sink) as Arc<dyn EventSink>,
        );
        spawn(actor.run(facts));

        let (requests, inbox) = channel(REQUESTS);
        let next_id = Arc::new(AtomicU32::new(1));
        let running = Arc::new(AtomicBool::new(false));
        let far_end_line = Arc::new(AtomicBool::new(false));
        spawn(
            Pump {
                transport,
                engine,
                tracker: BoundaryTracker::new(markers),
                grace: config.integration_grace,
                quiescence: config.quiescence,
                far_end_settle: config.far_end_settle,
                clock,
                reads,
                inbox,
                inputs,
                next_id: Arc::clone(&next_id),
                running: Arc::clone(&running),
                integration: Integration::Pending,
                drawing: None,
                standing: None,
                spoken: false,
                setup,
                self_talk: None,
                discards_line,
                markers,
                submitted: VecDeque::new(),
                echo: Echo::default(),
                open: None,
                interrupted: false,
                barren: false,
                lines: HashMap::new(),
                held: None,
                row: String::new(),
                cursor: None,
                pending_row: None,
                far_end: FarEndLine::default(),
            }
            .run(),
        );

        Self {
            requests,
            next_id,
            running,
            far_end_line,
            eof: shell.eof,
            sink,
        }
    }
}

impl SessionApi for SessionService {
    fn attach_session(&self, _session: SessionId, sink: Arc<dyn EventSink>) {
        self.sink.attach(sink);
    }

    /// Returns `Accepted` even when the request channel is full or closed: the id was already
    /// minted.
    fn submit_command(&self, _session: SessionId, line: &str) -> SubmitAck {
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let accepted = self.requests.try_send(Request::Submit {
            command_id,
            line: line.to_owned(),
        });
        if accepted.is_ok() {
            self.running.store(true, Ordering::SeqCst);
        }
        SubmitAck::Accepted { command_id }
    }

    fn send_key(&self, _session: SessionId, key: KeyPress) -> KeyAck {
        let intent = match binding_for(&key, self.owner()) {
            Binding::Unbound => return KeyAck::Unbound,
            Binding::ToFarEnd => {
                return match self.requests.try_send(Request::Key { key }) {
                    Ok(()) => KeyAck::Applied,
                    Err(_) => KeyAck::NothingToActOn,
                };
            }
            Binding::Intent(intent) => intent,
        };
        match intent {
            SessionIntent::Interrupt => {
                if !self.running.load(Ordering::SeqCst) {
                    return KeyAck::NothingToActOn;
                }
                match self.requests.try_send(Request::Interrupt) {
                    Ok(()) => KeyAck::Applied,
                    Err(_) => KeyAck::NothingToActOn,
                }
            }
            SessionIntent::Eof => {
                let Some(bytes) = self.eof.clone() else {
                    return KeyAck::Unsupported;
                };
                match self.requests.try_send(Request::Eof { bytes }) {
                    Ok(()) => KeyAck::Applied,
                    Err(_) => KeyAck::NothingToActOn,
                }
            }
        }
    }

    /// The only writer of both copies of the line owner, so they cannot disagree.
    fn set_line_owner(&self, _session: SessionId, owner: LineOwner) {
        self.far_end_line
            .store(owner == LineOwner::FarEnd, Ordering::SeqCst);
        let _ = self.requests.try_send(Request::Owner(owner));
    }

    fn paste(&self, _session: SessionId, text: &str) {
        let _ = self.requests.try_send(Request::Paste {
            text: text.to_owned(),
        });
    }
}

impl SessionService {
    fn owner(&self) -> LineOwner {
        if self.far_end_line.load(Ordering::SeqCst) {
            LineOwner::FarEnd
        } else {
            LineOwner::Local
        }
    }
}

enum Request {
    Submit { command_id: CommandId, line: String },
    Interrupt,
    Eof { bytes: Vec<u8> },
    Key { key: KeyPress },
    Owner(LineOwner),
    Paste { text: String },
}

const BACKLOG: usize = 10_000;

#[derive(Default)]
struct Attached {
    sink: Option<Arc<dyn EventSink>>,
    backlog: Vec<SessionEvent>,
}

#[derive(Default)]
struct AttachedSink(Mutex<Attached>);

impl AttachedSink {
    fn attach(&self, sink: Arc<dyn EventSink>) {
        let mut attached = self.0.lock().expect("sink lock poisoned");
        attached.sink = Some(Arc::clone(&sink));
        for event in std::mem::take(&mut attached.backlog) {
            sink.send(event);
        }
    }
}

impl EventSink for AttachedSink {
    /// Forwards under the lock so no event can overtake a backlog being flushed; a sink must
    /// never send back into this one.
    fn send(&self, event: SessionEvent) {
        let mut attached = self.0.lock().expect("sink lock poisoned");
        match &attached.sink {
            Some(sink) => sink.send(event),
            None => {
                if attached.backlog.len() < BACKLOG {
                    attached.backlog.push(event);
                }
            }
        }
    }
}

struct Pump {
    transport: Box<dyn Transport>,
    engine: Box<dyn TerminalEngine + Send>,
    tracker: BoundaryTracker,
    grace: Duration,
    quiescence: Duration,
    far_end_settle: Duration,
    clock: Arc<dyn Clock>,
    reads: Receiver<Vec<u8>>,
    inbox: Receiver<Request>,
    inputs: UnboundedSender<SessionInput>,
    next_id: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    /// Mirrors the actor's integration status; the pump sends both copies' transitions, so they
    /// cannot disagree.
    integration: Integration,
    drawing: Option<String>,
    /// `None` once something has happened that makes the next prompt news; `readline` in `bash`
    /// re-emits the whole prompt, markers included, on every Tab, history recall and `Ctrl+L`.
    standing: Option<String>,
    spoken: bool,
    /// `None` for a far end that is not being set up.
    setup: Option<String>,
    /// While `Some`, what comes back is rendered and never announced; the setup block closing or
    /// the grace period expiring sets it to `None`.
    self_talk: Option<CommandId>,
    /// `None` for a shell whose line editor has no byte that discards the pending line.
    discards_line: Option<u8>,
    markers: ShellMarkers,
    submitted: VecDeque<Submitted>,
    echo: Echo,
    open: Option<CommandId>,
    interrupted: bool,
    /// Whether the open block is nobody's; set from what opened it rather than what it printed,
    /// so a real command that fails silently keeps its verdict.
    barren: bool,
    /// Kept across regions and blocks; an entry leaves only when [`Pump::due`] sees its line
    /// settle.
    lines: HashMap<LineId, Row>,
    held: Option<Held>,
    row: String,
    cursor: Option<LineId>,
    /// Only meaningful while a submission is pending; read it through [`Pump::pending_echo`].
    pending_row: Option<LineId>,
    far_end: FarEndLine,
}

#[derive(Debug, Default)]
struct Row {
    owed: bool,
    text: String,
}

struct Held {
    line: LineId,
    text: String,
    revision: LineRevision,
    spoken: bool,
}

impl Held {
    fn of(line: LineId, due: Due) -> Self {
        Self {
            line,
            text: due.text,
            revision: due.revision,
            spoken: due.spoken,
        }
    }

    fn absorb(&mut self, due: Due) {
        match due.revision {
            LineRevision::Appended => self.text.push_str(&due.text),
            _ => {
                self.text = due.text;
                self.revision = due.revision;
            }
        }
        self.spoken |= due.spoken;
    }

    fn due(self) -> Due {
        Due {
            text: self.text,
            revision: self.revision,
            spoken: self.spoken,
        }
    }
}

#[derive(Debug)]
struct Due {
    text: String,
    revision: LineRevision,
    spoken: bool,
}

/// Every field is inert while Acter owns the line.
#[derive(Debug, Default)]
struct FarEndLine {
    owner: LineOwner,
    anchor: Option<Anchor>,
    /// Whether a submission cleared the anchor; a far end that hides its cursor also has none.
    awaiting_prompt: bool,
    changed: Vec<RowChange>,
    watching: bool,
    /// The far end's cursor when the key went out; `None` if it was hidden.
    was: Option<Caret>,
    /// Rows the far end drew at its own prompt that the region filter turned away.
    printed: Vec<(LineId, Due)>,
    /// The text the listener currently has in front of them.
    held: String,
}

impl Pump {
    async fn run(mut self) {
        let mut grace = None;
        // Armed only while the far end owns the line.
        let mut settled = None;
        loop {
            // Resolved before any state is touched, so no timer future is alive while
            // the step below mutates the pump.
            let woke = select! {
                read = self.reads.recv() => Woke::Read(read),
                request = self.inbox.recv() => Woke::Request(request),
                () = fire(&mut grace) => Woke::Grace,
                () = fire(&mut settled) => Woke::Settled,
            };
            match woke {
                // The transport's channel closing is the end of the session, not an error.
                Woke::Read(None) => {
                    self.send(SessionInput::Connection {
                        state: ConnectionState::Disconnected,
                    });
                    break;
                }
                Woke::Read(Some(bytes)) => {
                    let first = !self.spoken;
                    if first {
                        self.spoken = true;
                        self.send(SessionInput::Connection {
                            state: ConnectionState::Connected,
                        });
                    }
                    if first {
                        grace = Some(self.clock.timer(self.grace));
                    }
                    self.feed(&bytes).await;
                    self.set_up().await;
                    // Re-armed on every read; after Enter the next prompt may still be drawing,
                    // so only a key the reader is polling for settles on `far_end_settle`.
                    if self.far_end.owner == LineOwner::FarEnd {
                        let waiting_on_a_key =
                            self.far_end.watching && !self.far_end.awaiting_prompt;
                        let wait = if waiting_on_a_key {
                            self.far_end_settle
                        } else {
                            self.quiescence
                        };
                        settled = Some(self.clock.timer(wait));
                    }
                }
                Woke::Request(None) => break,
                Woke::Request(Some(request)) => self.request(request).await,
                Woke::Grace => {
                    grace = None;
                    self.grace_expired().await;
                }
                Woke::Settled => {
                    settled = None;
                    self.far_end_settled();
                }
            }
        }
    }

    async fn feed(&mut self, bytes: &[u8]) {
        let items = self.engine.advance(bytes);
        for event in self.tracker.observe(items) {
            match event {
                BoundaryEvent::MarkersObserved => {
                    self.integration = self.integration.markers_observed();
                    self.send(SessionInput::MarkersObserved);
                }
                BoundaryEvent::BlockStarted => {
                    self.standing = None;
                    self.block_started().await;
                }
                BoundaryEvent::Line {
                    region,
                    id,
                    text,
                    revision,
                } => self.line(region, id, text, revision).await,
                BoundaryEvent::BlockEnded { exit } => {
                    self.standing = None;
                    self.close(exit).await;
                }
                BoundaryEvent::ScreenChanged(Screen::Alternate) => {
                    self.send(SessionInput::AltScreenEntered);
                }
                BoundaryEvent::ScreenChanged(Screen::Normal) => {
                    self.send(SessionInput::AltScreenLeft);
                }
            }
            // Inside the loop, so a prompt is spoken before output later in the same read.
            self.prompt_finished();
        }
        // A read holding only `B` produces no event, yet it is what finished the prompt.
        self.prompt_finished();

        // A program that sent a device query waits forever unless the answer is written back.
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            self.write(&replies);
        }
    }

    /// Waits for a cursor, not just a first byte: `bash` on Ubuntu 24.04 under WSL sends a first
    /// read that draws no line, and a setup sent then has its echo held together with the prompt.
    async fn set_up(&mut self) {
        if self.cursor.is_none() {
            return;
        }
        let Some(line) = self.setup.take() else {
            return;
        };
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        // Before the write, so a prompt already drawn and not yet read is quieted too.
        self.self_talk = Some(command_id);
        self.send(SessionInput::SelfTalk(true));
        self.submit_line(command_id, &line, true).await;
    }

    async fn request(&mut self, request: Request) {
        match request {
            Request::Submit { command_id, line } => self.submit(command_id, &line).await,
            Request::Interrupt => self.interrupt(),
            Request::Eof { bytes } => self.end_input(&bytes),
            Request::Key { key } => self.far_end_key(key).await,
            Request::Owner(owner) => self.line_owner(owner),
            Request::Paste { text } => self.paste(&text),
        }
    }

    fn end_input(&mut self, bytes: &[u8]) {
        self.write(bytes);
    }

    /// The anchor is taken here because a far end sitting at its prompt sends nothing more to
    /// settle on.
    fn line_owner(&mut self, owner: LineOwner) {
        self.far_end.owner = owner;
        self.far_end.changed.clear();
        self.far_end.watching = false;
        self.far_end.awaiting_prompt = false;
        self.far_end.was = None;
        self.far_end.held.clear();
        self.far_end.printed.clear();
        match owner {
            LineOwner::FarEnd => self.anchor_here(),
            LineOwner::Local => self.far_end.anchor = None,
        }
    }

    async fn far_end_key(&mut self, key: KeyPress) {
        let bytes = key_bytes(&key, self.engine.modes());
        if key.key == Key::Enter {
            self.far_end_submitted().await;
        }
        self.far_end.was = self.caret();
        self.far_end.changed.clear();
        self.far_end.watching = true;
        self.write(&bytes);
    }

    async fn far_end_submitted(&mut self) {
        let line = self.row_from_anchor().trim().to_owned();
        self.far_end.anchor = None;
        self.far_end.awaiting_prompt = true;
        if line.is_empty() {
            return;
        }
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        // Opened before the bytes go out: the actor drops output arriving while nothing is active.
        self.close(None).await;
        self.open(command_id, Some(line));
        self.settle_running();
    }

    fn paste(&mut self, text: &str) {
        if self.far_end.owner != LineOwner::FarEnd {
            return;
        }
        let mut bytes = Vec::new();
        if self.engine.modes().bracketed_paste {
            bytes.extend_from_slice(BRACKET_START);
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(BRACKET_END);
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        self.write(&bytes);
    }

    fn far_end_settled(&mut self) {
        if self.far_end.owner != LineOwner::FarEnd {
            return;
        }
        self.printed();
        let changed = std::mem::take(&mut self.far_end.changed);
        let watching = std::mem::take(&mut self.far_end.watching);
        if !watching || std::mem::take(&mut self.far_end.awaiting_prompt) {
            self.anchor_here();
            return;
        }
        self.follow_cursor();
        let answer = far_end_row(&Keystroke {
            changed: &changed,
            anchor: self.far_end.anchor,
            was: self.far_end.was,
            now: self.caret(),
            held: &self.far_end.held,
        });
        match answer {
            FarEndAnswer::Row { text, caret } => self.far_end_line(Some(text), caret),
            FarEndAnswer::Caret { caret } => self.far_end_line(None, caret),
            FarEndAnswer::Nothing => {}
        }
    }

    /// Publishes every row except the cursor's, which is the command line and belongs to the field.
    fn printed(&mut self) {
        let printed = std::mem::take(&mut self.far_end.printed);
        for (id, due) in printed {
            if self.cursor == Some(id) {
                continue;
            }
            self.publish(id, due);
        }
    }

    /// `readline` in `bash` redraws the command line on a new row after listing completions; the
    /// new anchor is where what the listener already had begins on the cursor's row.
    fn follow_cursor(&mut self) {
        if !self.engine.cursor().visible {
            return;
        }
        let Some(line) = self.cursor else {
            return;
        };
        if self
            .far_end
            .anchor
            .is_some_and(|anchor| anchor.line == line)
        {
            return;
        }
        let held = self.far_end.held.trim_end().to_owned();
        if held.is_empty() {
            return;
        }
        let row = self.row_text(line);
        let row = row.trim_end();
        if !row.ends_with(&held) {
            return;
        }
        let column = row.chars().count() - held.chars().count();
        self.far_end.anchor = Some(Anchor {
            line,
            column: u16::try_from(column).unwrap_or(u16::MAX),
        });
    }

    /// Nothing is taken from a hidden cursor: `gh` hides it for a selection prompt and parks it on
    /// the blank row below the options.
    fn anchor_here(&mut self) {
        let cursor = self.engine.cursor();
        let Some(line) = self.cursor.filter(|_| cursor.visible) else {
            return;
        };
        self.far_end.anchor = Some(Anchor {
            line,
            column: cursor.column,
        });
        let text = self.row_from_anchor();
        self.far_end_line(Some(text), 0);
    }

    fn row_from_anchor(&self) -> String {
        let Some(anchor) = self.far_end.anchor else {
            return String::new();
        };
        self.lines
            .get(&anchor.line)
            .map(|row| row.text.chars().skip(usize::from(anchor.column)).collect())
            .unwrap_or_default()
    }

    /// `None` while the far end hides its cursor.
    fn caret(&self) -> Option<Caret> {
        let cursor = self.engine.cursor();
        cursor.visible.then_some(Caret {
            column: cursor.column,
            row: cursor.row,
        })
    }

    fn far_end_line(&mut self, text: Option<String>, caret: usize) {
        if let Some(text) = text.as_ref() {
            self.far_end.held = text.clone();
        }
        self.send(SessionInput::FarEndLine {
            text,
            caret: u32::try_from(caret).unwrap_or(u32::MAX),
        });
    }

    /// Decides speech only; the anchored row still reaches the buffer.
    fn on_anchor(&self, id: LineId) -> bool {
        self.far_end.owner == LineOwner::FarEnd
            && self.far_end.anchor.is_some_and(|anchor| anchor.line == id)
    }

    fn note_change(&mut self, id: LineId, before: String, text: &str, revision: LineRevision) {
        if self.far_end.owner != LineOwner::FarEnd || !self.far_end.watching {
            return;
        }
        let after = match revision {
            LineRevision::Appended => format!("{before}{text}"),
            _ => text.to_owned(),
        };
        match self
            .far_end
            .changed
            .iter_mut()
            .find(|change| change.line == id)
        {
            Some(change) => change.after = after,
            None => self.far_end.changed.push(RowChange {
                line: id,
                before,
                after,
            }),
        }
    }

    fn row_text(&self, id: LineId) -> String {
        self.lines
            .get(&id)
            .map_or_else(String::new, |row| row.text.clone())
    }

    async fn submit(&mut self, command_id: CommandId, line: &str) {
        self.submit_line(command_id, line, false).await;
    }

    async fn submit_line(&mut self, command_id: CommandId, line: &str, ours: bool) {
        // Before the queue is touched, so `submitted.is_empty()` still means nothing else was
        // pending and the discard byte goes out ahead of the line it protects.
        self.cancel_pending_input();
        if !line.trim().is_empty() {
            self.submitted.push_back(Submitted {
                id: command_id,
                line: line.to_owned(),
                ours,
                minted: false,
            });
            self.pending_row = self.cursor;
        }
        self.write(format!("{line}{ENTER}").as_bytes());
        self.settle_running();
    }

    /// Throws away a cursor-position answer ConPTY queued into `cmd.exe`'s input itself, which
    /// would otherwise be read in front of the submitted line.
    ///
    /// Only at a prompt with nothing else pending, since the byte is a keypress to a program
    /// reading raw input; and always as its own write, because ConPTY reads an escape followed
    /// by a letter as `Alt` plus that letter.
    fn cancel_pending_input(&mut self) {
        let Some(cancel) = self.discards_line else {
            return;
        };
        let reading = matches!(self.tracker.region(), Region::Prompt | Region::CommandLine);
        if reading && self.submitted.is_empty() {
            self.write(&[cancel]);
        }
    }

    /// Closes nothing: the actor drops output arriving with no command active, and the prompt that
    /// comes back after an interrupt is the only answer the user gets.
    fn interrupt(&mut self) {
        self.interrupted = true;
        // A failed interrupt means the far end is gone, which the closing read channel reports.
        let _ = self.transport.interrupt();
    }

    /// The echo is taken whether or not it is used, so it is never offered to the next block.
    async fn block_started(&mut self) {
        let echoed = self.echo.take();
        if self.open.is_some() && self.submitted.is_empty() {
            return;
        }
        self.close(None).await;
        let claimed = self.claim(echoed.as_deref());
        let named = echoed.or_else(|| {
            claimed
                .ours
                .then_some(claimed.line)
                .filter(|line| !line.trim().is_empty())
        });
        let nobodys = claimed.minted && named.is_none();
        self.open(claimed.id, named);
        self.barren = nobodys;
        self.settle_running();
    }

    fn drawn(&mut self, region: Region, text: &str, revision: LineRevision) {
        if !self.markers.reports_exit_code() {
            return;
        }
        match region {
            Region::Prompt => {
                let drawing = self.drawing.get_or_insert_with(String::new);
                match revision {
                    LineRevision::Appended => drawing.push_str(text),
                    LineRevision::Rewritten | LineRevision::Settled => {
                        drawing.clear();
                        drawing.push_str(text);
                    }
                }
            }
            _ => self.prompt_finished(),
        }
    }

    /// A repaint is told apart from a new prompt by whether a command started or ended since,
    /// never by the text: only that clears [`Pump::standing`].
    fn prompt_finished(&mut self) {
        if self.tracker.region() == Region::Prompt {
            return;
        }
        let Some(drawn) = self.drawing.take() else {
            return;
        };
        if drawn.trim().is_empty() || self.standing.as_deref() == Some(drawn.as_str()) {
            return;
        }
        self.standing = Some(drawn.clone());
        self.send(SessionInput::PromptDrawn { text: drawn });
    }

    async fn line(&mut self, region: Region, id: LineId, text: String, revision: LineRevision) {
        self.drawn(region, &text, revision);

        self.echo.observe(region, &text, revision);

        if revision != LineRevision::Settled {
            self.cursor = Some(id);
        }

        // Read before `due` writes the row: the far-end rule compares what it said when the key
        // went out.
        let before = self.row_text(id);
        // `due` runs whatever the region: its bookkeeping tells the echo's row from output later.
        let due = self.due(id, text.clone(), revision);
        self.note_change(id, before, &text, revision);

        if let Some(due) = due {
            if self.wants(region) {
                // Held with no block open, since the actor drops text while nothing is active,
                // and on the row a submission is pending on, since what lands there is its echo.
                match self.open {
                    Some(_) if !self.pending_echo(id) => self.output(id, due),
                    _ => self.hold(id, due).await,
                }
            } else if self.far_end.owner == LineOwner::FarEnd {
                // Kept until the batch settles, when it is known which row was the command line.
                self.far_end.printed.push((id, due));
            }
        }

        self.boundary(region, id, text, revision).await;
        if self.window().is_none() {
            self.spill().await;
        }
    }

    /// Bounded by [`Pump::window`] and by a submission being pending; text past either can no
    /// longer be an echo and is spilled.
    async fn hold(&mut self, id: LineId, due: Due) {
        let Some(window) = self.window() else {
            self.spill().await;
            self.publish(id, due);
            return;
        };
        if !self.held.as_ref().is_some_and(|held| held.line == id) {
            self.spill().await;
        }
        match self.held.as_mut() {
            Some(held) => held.absorb(due),
            None => self.held = Some(Held::of(id, due)),
        }
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.text.len() > window)
        {
            self.spill().await;
        }
    }

    fn pending_echo(&self, id: LineId) -> bool {
        self.pending_row == Some(id) && self.window().is_some()
    }

    async fn spill(&mut self) {
        let Some(held) = self.held.take() else {
            return;
        };
        self.publish(held.line, held.due());
    }

    fn publish(&mut self, id: LineId, due: Due) {
        if self.open.is_none() {
            self.unclaimed();
        }
        self.output(id, due);
    }

    /// A fresh id, never [`Pump::claim`]: claiming would take the submission whose echo this row
    /// may be about to complete.
    fn unclaimed(&mut self) {
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.open(command_id, None);
        self.barren = true;
        self.settle_running();
    }

    /// Decided on the row's accumulated text, never on one append: a pseudoconsole can split
    /// `dir /s` across any number of reads.
    async fn boundary(&mut self, region: Region, id: LineId, text: String, revision: LineRevision) {
        if region == Region::CommandLine {
            self.row.clear();
            return;
        }
        let Some(window) = self.window() else {
            self.row.clear();
            return;
        };

        // A rewrite or a settlement carries the whole row, so it replaces what accumulated.
        match revision {
            LineRevision::Appended => self.row.push_str(&text),
            _ if self.pending_row == Some(id) => self.row = text.clone(),
            _ => {
                self.row.clear();
                return;
            }
        }
        if self.row.len() > window {
            let over = self.row.len() - window;
            let cut = (over..=self.row.len())
                .find(|at| self.row.is_char_boundary(*at))
                .unwrap_or(self.row.len());
            self.row.drain(..cut);
        }

        let Some(index) = self.echoed(&self.row) else {
            return;
        };
        let submitted = self.adopt(index);

        // Held text that is this echo is dropped; whatever preceded the echo on that row still
        // reaches a block.
        let whole = (revision != LineRevision::Appended).then_some(text.as_str());
        if let Some(held) = self.held.take() {
            let line = held.line;
            match before_echo(&held.text, whole, submitted.line.trim()) {
                Some(before) if before.is_empty() => {}
                Some(before) => {
                    let mut due = held.due();
                    due.text = before;
                    self.publish(line, due);
                }
                None => self.publish(line, held.due()),
            }
        }

        self.close(None).await;
        self.open(submitted.id, Some(submitted.line));
        self.settle_running();
        self.row.clear();
    }

    /// The longest pending line plus the one character that proves a match starts a word; `None`
    /// when nothing is waiting for an echo.
    fn window(&self) -> Option<usize> {
        self.submitted
            .iter()
            .map(|submitted| submitted.line.trim().len())
            .max()
            .filter(|longest| *longest > 0)
            .map(|longest| longest + 1)
    }

    fn echoed(&self, row: &str) -> Option<usize> {
        let row = row.trim_end();
        self.submitted.iter().position(|submitted| {
            let line = submitted.line.trim();
            !line.is_empty()
                && row.ends_with(line)
                && row[..row.len() - line.len()]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric())
        })
    }

    fn adopt(&mut self, index: usize) -> Submitted {
        self.submitted.drain(..index);
        self.submitted
            .pop_front()
            .expect("the index came from this queue")
    }

    /// Retires the submissions ahead of the one the shell echoed, which assumes a serial shell has
    /// already disposed of the earlier lines.
    fn claim(&mut self, echoed: Option<&str>) -> Submitted {
        if let Some(echoed) = echoed
            && let Some(index) = self
                .submitted
                .iter()
                .position(|submitted| submitted.line.trim() == echoed)
        {
            self.submitted.drain(..index);
        }
        self.submitted.pop_front().unwrap_or_else(|| Submitted {
            id: CommandId(self.next_id.fetch_add(1, Ordering::SeqCst)),
            line: String::new(),
            ours: false,
            minted: true,
        })
    }

    fn open(&mut self, command_id: CommandId, command_line: Option<String>) {
        self.open = Some(command_id);
        self.barren = false;
        self.send(SessionInput::CommandStarted {
            command_id,
            command_line,
        });
    }

    async fn close(&mut self, exit: Option<ExitCode>) {
        let Some(command_id) = self.open.take() else {
            return;
        };
        // Every recorded line's text went to the closing block; clearing the map instead would make
        // their later settlements look like lines never seen.
        self.lines.values_mut().for_each(|row| row.owed = false);
        let stopped = exit.is_none() && self.interrupted;
        self.interrupted = false;
        self.send(if stopped {
            SessionInput::CommandInterrupted { command_id }
        } else if self.barren {
            SessionInput::NothingRan { command_id }
        } else {
            SessionInput::CommandEnded {
                command_id,
                exit_code: exit.unwrap_or(ExitCode(0)),
            }
        });
        // After the ending, never before: the flush the ending triggers must be quiet too.
        if self.self_talk == Some(command_id) {
            self.self_talk = None;
            self.send(SessionInput::SelfTalk(false));
        }
        self.settle_running();
    }

    async fn grace_expired(&mut self) {
        self.integration = self.integration.grace_period_expired();
        self.send(SessionInput::GracePeriodExpired);
        // A setup whose markers never arrive closes no block, so only this turns speech back on.
        if self.self_talk.take().is_some() {
            self.send(SessionInput::SelfTalk(false));
        }
        self.settle_running();
    }

    /// Before the first marker every line is `Unstructured`, so a pending session must want it or
    /// the first prompt is lost.
    fn wants(&self, region: Region) -> bool {
        match self.integration {
            Integration::Unintegrated => region == Region::Unstructured,
            Integration::Pending => region == Region::Unstructured || self.marked(region),
            Integration::Integrated => self.marked(region),
        }
    }

    fn marked(&self, region: Region) -> bool {
        // A shell with no exit code ends a command only by drawing its prompt, so the prompt is
        // content.
        if self.markers.reports_exit_code() {
            return region == Region::Output;
        }
        matches!(region, Region::Output | Region::Prompt)
    }

    fn due(&mut self, id: LineId, text: String, revision: LineRevision) -> Option<Due> {
        match revision {
            LineRevision::Appended => {
                let row = self.lines.entry(id).or_default();
                row.owed = false;
                row.text.push_str(&text);
                Some(Due {
                    text,
                    revision,
                    spoken: true,
                })
            }
            LineRevision::Rewritten => {
                let row = self.lines.entry(id).or_default();
                row.owed = true;
                row.text = text.clone();
                Some(Due {
                    text,
                    revision,
                    spoken: false,
                })
            }
            // Owed when rewritten since its last word, or never seen: a line that scrolled out
            // within one read arrives settled without ever appending.
            LineRevision::Settled => {
                self.lines
                    .remove(&id)
                    .is_none_or(|row| row.owed)
                    .then_some(Due {
                        text,
                        revision,
                        spoken: true,
                    })
            }
        }
    }

    fn output(&mut self, id: LineId, due: Due) {
        self.barren = false;
        let spoken = due.spoken && !self.on_anchor(id);
        self.send(SessionInput::Output {
            line: id,
            revision: due.revision,
            text: due.text,
            spoken,
        });
    }

    fn settle_running(&self) {
        self.running.store(
            self.open.is_some() || !self.submitted.is_empty(),
            Ordering::SeqCst,
        );
    }

    /// A failure means the far end is gone, which the read channel closing reports.
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.transport.write(bytes);
    }

    /// A closed channel means the actor is gone, which happens only at teardown.
    fn send(&self, input: SessionInput) {
        let _ = self.inputs.send(input);
    }
}

/// `None` means none of the held text could be told apart, and the caller publishes it whole.
fn before_echo(held: &str, row: Option<&str>, line: &str) -> Option<String> {
    let held = held.trim_end();
    let Some(row) = row.map(str::trim_end) else {
        return held.strip_suffix(line).map(str::to_owned);
    };
    let echo_at = row.len().checked_sub(line.len())?;
    // `None` when something rewrote the row underneath the held text.
    let held_at = row.rfind(held)?;
    let keep = echo_at.saturating_sub(held_at).min(held.len());
    Some(held[..keep].to_owned())
}

struct Submitted {
    id: CommandId,
    line: String,
    /// A line the user typed is already headed by the frontend, so only Acter's own is named here.
    ours: bool,
    minted: bool,
}

#[derive(Default)]
struct Echo {
    prompt: String,
    /// `None` once a whole row arrived that does not start with the prompt.
    text: Option<String>,
}

impl Echo {
    fn observe(&mut self, region: Region, text: &str, revision: LineRevision) {
        match region {
            Region::CommandLine => match revision {
                LineRevision::Appended => {
                    if let Some(echoed) = self.text.as_mut() {
                        echoed.push_str(text);
                    }
                }
                LineRevision::Rewritten | LineRevision::Settled => {
                    self.text = text.strip_prefix(self.prompt.as_str()).map(str::to_owned);
                }
            },
            Region::Prompt => {
                self.text = Some(String::new());
                match revision {
                    LineRevision::Appended => self.prompt.push_str(text),
                    LineRevision::Rewritten | LineRevision::Settled => {
                        self.prompt = text.to_owned();
                    }
                }
            }
            Region::Output | Region::Unstructured => {
                self.text = Some(String::new());
                self.prompt.clear();
            }
        }
    }

    fn take(&mut self) -> Option<String> {
        let text = self.text.take().map(|text| text.trim().to_owned());
        self.text = Some(String::new());
        self.prompt.clear();
        text.filter(|text| !text.is_empty())
    }
}

enum Woke {
    Read(Option<Vec<u8>>),
    Request(Option<Request>),
    Grace,
    Settled,
}

async fn fire(timer: &mut Option<Timer>) {
    match timer {
        Some(timer) => timer.await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;
    use tokio::task::yield_now;

    use crate::{
        Announcement, Cursor, Key, Osc133Marker, SessionSetup, TerminalItem, TerminalModes,
        TransportError,
    };

    use super::*;

    #[derive(Default)]
    struct FakeClock {
        now: Mutex<Duration>,
        armed: Mutex<Vec<(Duration, oneshot::Sender<()>)>>,
    }

    impl FakeClock {
        fn advance_to(&self, at: Duration) {
            *self.now.lock().expect("clock poisoned") = at;
            let mut armed = self.armed.lock().expect("timers poisoned");
            let (due, pending) = std::mem::take(&mut *armed)
                .into_iter()
                .partition::<Vec<_>, _>(|(deadline, _)| *deadline <= at);
            *armed = pending;
            for (_, fire) in due {
                let _ = fire.send(());
            }
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

    struct FarEnd {
        reads: Mutex<Option<Sender<Vec<u8>>>>,
        batches: Mutex<VecDeque<Vec<TerminalItem>>>,
        written: Mutex<Vec<u8>>,
        interrupts: Mutex<u32>,
        cursor: Mutex<Cursor>,
        modes: Mutex<TerminalModes>,
    }

    impl Default for FarEnd {
        fn default() -> Self {
            Self {
                reads: Mutex::new(None),
                batches: Mutex::new(VecDeque::new()),
                written: Mutex::new(Vec::new()),
                interrupts: Mutex::new(0),
                cursor: Mutex::new(Cursor {
                    column: 0,
                    row: 0,
                    visible: true,
                }),
                modes: Mutex::new(TerminalModes {
                    application_cursor_keys: false,
                    bracketed_paste: false,
                }),
            }
        }
    }

    struct FakeTransport(Arc<FarEnd>);

    impl Transport for FakeTransport {
        fn start(&mut self, bytes: Sender<Vec<u8>>) {
            *self.0.reads.lock().expect("far end poisoned") = Some(bytes);
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
            self.0
                .written
                .lock()
                .expect("far end poisoned")
                .extend_from_slice(bytes);
            Ok(())
        }

        fn interrupt(&mut self) -> Result<(), TransportError> {
            *self.0.interrupts.lock().expect("far end poisoned") += 1;
            Ok(())
        }

        fn resize(&mut self, _columns: u16, _screen_lines: u16) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct FakeEngine(Arc<FarEnd>);

    impl TerminalEngine for FakeEngine {
        fn advance(&mut self, _bytes: &[u8]) -> Vec<TerminalItem> {
            self.0
                .batches
                .lock()
                .expect("far end poisoned")
                .pop_front()
                .unwrap_or_default()
        }

        fn screen(&self) -> Screen {
            Screen::Normal
        }

        fn resize(&mut self, _columns: u16, _screen_lines: u16) {}

        fn take_replies(&mut self) -> Vec<u8> {
            Vec::new()
        }

        fn cursor(&self) -> Cursor {
            *self.0.cursor.lock().expect("far end poisoned")
        }

        fn modes(&self) -> TerminalModes {
            *self.0.modes.lock().expect("far end poisoned")
        }
    }

    fn marking(markers: ShellMarkers) -> ShellFacts {
        ShellFacts {
            markers,
            eof: None,
            setup: None,
            discards_line: None,
        }
    }

    fn discarding_on(byte: u8) -> ShellFacts {
        ShellFacts {
            discards_line: Some(byte),
            ..marking(ShellMarkers::PromptAndCommandLine)
        }
    }

    fn set_up_with(line: &str) -> ShellFacts {
        ShellFacts {
            setup: Some(SessionSetup {
                line: line.to_owned(),
                markers: ShellMarkers::Full,
            }),
            ..marking(ShellMarkers::Full)
        }
    }

    /// Deliberately not PowerShell's measured answer, so a service that hardcoded it would fail.
    fn ending_with(bytes: &[u8]) -> ShellFacts {
        ShellFacts {
            markers: ShellMarkers::Full,
            eof: Some(bytes.to_vec()),
            setup: None,
            discards_line: None,
        }
    }

    struct Session {
        api: SessionService,
        clock: Arc<FakeClock>,
        events: Arc<Recorder>,
        far_end: Arc<FarEnd>,
    }

    impl Session {
        async fn start() -> Self {
            Self::with_config(PacingConfig::default()).await
        }

        async fn with_config(config: PacingConfig) -> Self {
            Self::of(config, ShellMarkers::Full).await
        }

        async fn of(config: PacingConfig, markers: ShellMarkers) -> Self {
            Self::over(config, marking(markers)).await
        }

        async fn over(config: PacingConfig, shell: ShellFacts) -> Self {
            let far_end = Arc::new(FarEnd::default());
            let clock = Arc::new(FakeClock::default());
            let events = Arc::new(Recorder::default());
            let api = SessionService::start(
                Box::new(FakeTransport(Arc::clone(&far_end))),
                Box::new(FakeEngine(Arc::clone(&far_end))),
                Arc::clone(&clock) as Arc<dyn Clock>,
                config,
                shell,
            );
            api.attach_session(SessionId(1), Arc::clone(&events) as Arc<dyn EventSink>);
            let session = Self {
                api,
                clock,
                events,
                far_end,
            };
            session.settle().await;
            session
        }

        async fn settle(&self) {
            for _ in 0..64 {
                yield_now().await;
            }
        }

        async fn emit(&self, items: Vec<TerminalItem>) {
            self.far_end
                .batches
                .lock()
                .expect("far end poisoned")
                .push_back(items);
            let reads = self.far_end.reads.lock().expect("far end poisoned").clone();
            reads
                .expect("the session was started")
                .try_send(vec![b'.'])
                .expect("the read channel has room");
            self.settle().await;
        }

        async fn submit(&self, line: &str) -> CommandId {
            let ack = self.api.submit_command(SessionId(1), line);
            self.settle().await;
            match ack {
                SubmitAck::Accepted { command_id } => command_id,
                SubmitAck::NotConnected => panic!("a running session accepts a line"),
            }
        }

        async fn press(&self, key: KeyPress) -> KeyAck {
            let ack = self.api.send_key(SessionId(1), key);
            self.settle().await;
            ack
        }

        async fn cursor_at(&self, column: u16, row: u16) {
            *self.far_end.cursor.lock().expect("far end poisoned") = Cursor {
                column,
                row,
                visible: true,
            };
            self.settle().await;
        }

        async fn hides_its_cursor(&self) {
            self.far_end
                .cursor
                .lock()
                .expect("far end poisoned")
                .visible = false;
            self.settle().await;
        }

        async fn asks_for(&self, modes: TerminalModes) {
            *self.far_end.modes.lock().expect("far end poisoned") = modes;
            self.settle().await;
        }

        async fn owner(&self, owner: LineOwner) {
            self.api.set_line_owner(SessionId(1), owner);
            self.settle().await;
        }

        fn far_end_lines(&self) -> Vec<(Option<String>, u32)> {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::FarEndLine { text, caret } => Some((text, caret)),
                    _ => None,
                })
                .collect()
        }

        async fn advance_to(&self, millis: u64) {
            self.clock.advance_to(Duration::from_millis(millis));
            self.settle().await;
        }

        fn events(&self) -> Vec<SessionEvent> {
            self.events.0.lock().expect("recorder poisoned").clone()
        }

        fn started(&self) -> Vec<CommandId> {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::CommandStarted { command_id, .. } => Some(command_id),
                    _ => None,
                })
                .collect()
        }

        fn headings(&self) -> Vec<Option<String>> {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::CommandStarted { command_line, .. } => Some(command_line),
                    _ => None,
                })
                .collect()
        }

        fn output_of(&self, command_id: CommandId) -> String {
            joined(self.events().into_iter().filter(|event| {
                !matches!(event, SessionEvent::Output { command_id: at, .. } if *at != command_id)
            }))
        }

        fn outputs(&self) -> Vec<String> {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::Output { text, .. } => Some(text),
                    _ => None,
                })
                .collect()
        }

        fn rendered(&self) -> String {
            joined(self.events())
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

        fn written(&self) -> String {
            String::from_utf8_lossy(&self.far_end.written.lock().expect("far end poisoned"))
                .into_owned()
        }

        fn interrupts(&self) -> u32 {
            *self.far_end.interrupts.lock().expect("far end poisoned")
        }
    }

    fn joined(events: impl IntoIterator<Item = SessionEvent>) -> String {
        let mut text = String::new();
        let mut last: Option<LineId> = None;
        for event in events {
            match event {
                SessionEvent::Output {
                    line,
                    revision,
                    text: chunk,
                    ..
                } => {
                    if last.is_some_and(|last| last != line) {
                        text.push('\n');
                    }
                    last = Some(line);
                    match revision {
                        LineRevision::Appended => text.push_str(&chunk),
                        _ => {
                            let start = text.len() - text.rsplit('\n').next().unwrap_or("").len();
                            text.truncate(start);
                            text.push_str(&chunk);
                        }
                    }
                }
                SessionEvent::CommandStarted { .. }
                | SessionEvent::CommandFinished { .. }
                | SessionEvent::CommandInterrupted { .. } => last = None,
                _ => {}
            }
        }
        text
    }

    const PROMPT: &str = r"C:\>";

    fn marker(marker: Osc133Marker) -> TerminalItem {
        TerminalItem::Marker(marker)
    }

    fn line(id: u64, text: &str) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision: LineRevision::Appended,
        }
    }

    fn rewritten(id: u64, text: &str) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision: LineRevision::Rewritten,
        }
    }

    fn settled(id: u64, text: &str) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision: LineRevision::Settled,
        }
    }

    fn command(id: u64, echo: &str, output: &str, exit: Option<i32>) -> Vec<TerminalItem> {
        vec![
            marker(Osc133Marker::PromptStart),
            line(id, "> "),
            marker(Osc133Marker::CommandStart),
            line(id, echo),
            marker(Osc133Marker::OutputStart),
            line(id + 1, output),
            marker(Osc133Marker::CommandEnd(exit.map(ExitCode))),
        ]
    }

    fn ctrl(letter: char) -> KeyPress {
        KeyPress {
            key: Key::Char(letter),
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    #[tokio::test]
    async fn the_text_is_rendered_before_anything_is_said_about_it() {
        let session = Session::start().await;

        session.submit("small").await;
        session
            .emit(command(1, "small", "hello from acter", Some(0)))
            .await;
        session.advance_to(1_000).await;

        let events = session.events();
        let rendered_at = events
            .iter()
            .position(|event| matches!(event, SessionEvent::Output { .. }))
            .unwrap_or_else(|| panic!("nothing was rendered: {events:?}"));
        let spoken_at = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    SessionEvent::Announce {
                        announcement: Announcement::ReadAloud { .. },
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("nothing was read aloud: {events:?}"));

        assert!(
            rendered_at < spoken_at,
            "the span was spoken before it was rendered: {events:?}"
        );
    }

    #[tokio::test]
    async fn blocks_claim_submitted_ids_in_the_order_they_were_submitted() {
        let session = Session::start().await;

        let first = session.submit("one").await;
        let second = session.submit("two").await;
        session
            .emit(command(1, "one", "first output", Some(0)))
            .await;
        session
            .emit(command(3, "two", "second output", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.started(), vec![first, second]);
        assert_eq!(session.written(), "one\rtwo\r");
    }

    #[tokio::test]
    async fn a_block_nobody_submitted_is_still_a_command_with_output() {
        let session = Session::start().await;

        session
            .emit(command(1, "", "output nobody asked for", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.started(), vec![CommandId(1)]);
        assert!(
            session.rendered().contains("output nobody asked for"),
            "the text reached the buffer: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    async fn a_submitted_line_ends_with_what_a_terminal_sends_for_enter() {
        let session = Session::start().await;

        session.submit("git status").await;

        assert_eq!(session.written(), "git status\r");
    }

    #[tokio::test]
    async fn an_id_the_shell_never_read_is_retired_by_the_echo_of_the_one_it_did() {
        let session = Session::start().await;

        let never_read = session.submit("typed into something else").await;
        let read = session.submit("runs").await;
        session.emit(command(1, "runs", "output", Some(0))).await;
        session.advance_to(1_000).await;

        assert_eq!(
            session.started(),
            vec![read],
            "the block is the line the shell echoed, not the one before it"
        );
        assert_eq!(session.rendered(), "output", "and its output went under it");

        let next = session.submit("after").await;
        session.emit(command(3, "after", "more", Some(0))).await;
        session.advance_to(2_000).await;

        assert_eq!(session.started(), vec![read, next]);
        assert!(
            !session.started().contains(&never_read),
            "the id the shell never read is claimed by no block at all: {:?}",
            session.started()
        );
    }

    #[tokio::test]
    async fn an_echo_that_matches_nothing_claims_the_front_of_the_queue() {
        let session = Session::start().await;

        let first = session.submit("one").await;
        session.submit("two").await;
        session
            .emit(command(1, "something else entirely", "output", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.started(), vec![first]);
    }

    #[tokio::test]
    async fn a_session_that_retired_an_id_can_say_there_is_nothing_to_stop() {
        let session = Session::start().await;

        session.submit("typed into something else").await;
        session.submit("runs").await;
        session.emit(command(1, "runs", "output", Some(0))).await;
        session.advance_to(1_000).await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
    }

    #[tokio::test]
    async fn a_block_is_headed_by_the_line_the_shell_echoed() {
        let session = Session::start().await;

        session.submit("git status").await;
        session
            .emit(command(1, "git status", "on branch main", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(
            session.headings(),
            vec![Some("git status".to_owned())],
            "{:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn an_echo_that_arrives_in_pieces_is_one_command_line() {
        let session = Session::start().await;

        session.submit("git status").await;
        session
            .emit(vec![
                marker(Osc133Marker::PromptStart),
                line(1, "> "),
                marker(Osc133Marker::CommandStart),
                line(1, "git "),
            ])
            .await;
        session
            .emit(vec![
                line(1, "status"),
                marker(Osc133Marker::OutputStart),
                line(2, "on branch main"),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.headings(), vec![Some("git status".to_owned())]);
    }

    #[tokio::test]
    async fn a_rewritten_echo_is_stripped_of_the_prompt_it_was_written_after() {
        let session = Session::start().await;

        session.submit("git status").await;
        session
            .emit(vec![
                marker(Osc133Marker::PromptStart),
                line(1, "> "),
                marker(Osc133Marker::CommandStart),
                rewritten(1, "> git status"),
                marker(Osc133Marker::OutputStart),
                line(2, "on branch main"),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.headings(), vec![Some("git status".to_owned())]);
    }

    #[tokio::test]
    async fn a_rewritten_echo_the_prompt_does_not_explain_is_unknown() {
        let session = Session::start().await;

        session.submit("git status").await;
        session
            .emit(vec![
                marker(Osc133Marker::PromptStart),
                line(1, "> "),
                marker(Osc133Marker::CommandStart),
                rewritten(1, "PS C:\\acter> git status"),
                marker(Osc133Marker::OutputStart),
                line(2, "on branch main"),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.headings(), vec![None]);
    }

    #[tokio::test]
    async fn a_block_with_no_echo_has_no_command_line() {
        let session = Session::start().await;

        session.submit("quiet").await;
        session
            .emit(vec![
                marker(Osc133Marker::PromptStart),
                line(1, "> "),
                marker(Osc133Marker::CommandStart),
                marker(Osc133Marker::OutputStart),
                line(2, "output"),
                marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.headings(), vec![None]);
    }

    #[tokio::test]
    async fn an_echo_never_carries_over_to_the_next_block() {
        let session = Session::start().await;

        session.submit("first").await;
        session.emit(command(1, "first", "output", Some(0))).await;
        session
            .emit(vec![
                marker(Osc133Marker::PromptStart),
                line(3, "> "),
                marker(Osc133Marker::CommandStart),
                marker(Osc133Marker::OutputStart),
                line(4, "more"),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.headings(), vec![Some("first".to_owned()), None]);
    }

    #[tokio::test]
    async fn the_prompt_and_the_echo_never_reach_the_frontend_as_output() {
        let session = Session::start().await;

        session.submit("hello").await;
        session
            .emit(command(1, "hello", "hello from acter", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.rendered(), "hello from acter");
    }

    #[tokio::test]
    async fn a_block_that_ends_with_no_code_after_an_interrupt_is_reported_as_stopped() {
        let session = Session::start().await;

        let command_id = session.submit("forever").await;
        session
            .emit(vec![marker(Osc133Marker::OutputStart), line(1, "working")])
            .await;
        assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);
        assert_eq!(
            session.interrupts(),
            1,
            "the transport was asked, not written to"
        );
        session
            .emit(vec![marker(Osc133Marker::CommandEnd(None))])
            .await;
        session.advance_to(1_000).await;

        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandInterrupted { command_id }),
            "{:?}",
            session.events()
        );
        assert!(
            !session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id }),
            "and never also as finished: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_block_that_ends_with_no_code_and_no_interrupt_still_finishes() {
        let session = Session::start().await;

        let command_id = session.submit("odd").await;
        session.emit(command(1, "odd", "output", None)).await;
        session.advance_to(1_000).await;

        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id }),
            "{:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn an_interrupt_does_not_outlive_the_command_it_was_aimed_at() {
        let session = Session::start().await;

        session.submit("forever").await;
        session.emit(vec![marker(Osc133Marker::OutputStart)]).await;
        session.press(ctrl('c')).await;
        session
            .emit(vec![marker(Osc133Marker::CommandEnd(None))])
            .await;

        session.submit("next").await;
        session.emit(command(2, "next", "output", None)).await;
        session.advance_to(1_000).await;

        assert_eq!(
            session
                .events()
                .iter()
                .filter(|event| matches!(event, SessionEvent::CommandInterrupted { .. }))
                .count(),
            1,
            "only the command that was stopped: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_key_nothing_is_bound_to_is_reported_as_unbound() {
        let session = Session::start().await;
        session.submit("running").await;
        session.emit(vec![marker(Osc133Marker::OutputStart)]).await;

        assert_eq!(session.press(ctrl('x')).await, KeyAck::Unbound);
        assert_eq!(
            session.interrupts(),
            0,
            "an unbound key reaches the far end as nothing at all"
        );
    }

    #[tokio::test]
    async fn a_bound_key_with_nothing_running_says_there_was_nothing_to_act_on() {
        let session = Session::start().await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
        assert_eq!(session.interrupts(), 0);
    }

    #[tokio::test]
    async fn a_submitted_command_can_be_stopped_before_its_block_opens() {
        let session = Session::start().await;

        session.submit("slow to start").await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);
        assert_eq!(session.interrupts(), 1);
    }

    #[tokio::test]
    async fn a_finished_command_leaves_nothing_to_act_on() {
        let session = Session::start().await;

        session.submit("quick").await;
        session.emit(command(1, "quick", "done", Some(0))).await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
        assert_eq!(session.interrupts(), 0);
    }

    fn quick_grace() -> PacingConfig {
        PacingConfig {
            integration_grace: Duration::from_millis(200),
            ..PacingConfig::default()
        }
    }

    #[tokio::test]
    async fn a_session_with_no_markers_is_flagged_when_the_grace_period_expires() {
        let session = Session::with_config(quick_grace()).await;

        session.emit(Vec::new()).await;
        session.advance_to(100).await;
        assert!(
            !session
                .events()
                .contains(&SessionEvent::IntegrationUnavailable),
            "nothing is said while the shell may still be marking: {:?}",
            session.events()
        );

        session.advance_to(300).await;
        assert!(
            session
                .events()
                .contains(&SessionEvent::IntegrationUnavailable)
        );
    }

    #[tokio::test]
    async fn a_far_end_that_has_not_spoken_yet_is_not_flagged_for_saying_nothing() {
        let session = Session::with_config(quick_grace()).await;

        session.advance_to(10_000).await;

        assert!(
            session.events().is_empty(),
            "a far end that is still starting has had no chance to mark anything: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn what_the_far_end_said_before_its_first_marker_reaches_the_frontend() {
        let session = Session::with_config(quick_grace()).await;

        session.emit(vec![line(1, "acter@acter-ssh:~$ ")]).await;
        session.advance_to(100).await;

        assert_eq!(
            session.rendered(),
            "acter@acter-ssh:~$ ",
            "the prompt the far end had already drawn: {:?}",
            session.events()
        );
        assert_eq!(
            session.started().len(),
            1,
            "in a block of its own, since no submission accounts for it: {:?}",
            session.events()
        );
        assert!(
            !session
                .events()
                .contains(&SessionEvent::IntegrationUnavailable),
            "and none of this is a verdict on the session yet: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_banner_printed_before_the_markers_is_kept_and_the_session_still_integrates() {
        let session = Session::with_config(quick_grace()).await;

        session
            .emit(vec![line(1, "Microsoft Windows [Version 10.0.26200.1]")])
            .await;
        let command_id = session.submit("echo hi").await;
        session.emit(command(2, "echo hi", "hi", Some(0))).await;
        session.advance_to(300).await;

        assert_eq!(
            session.rendered(),
            "Microsoft Windows [Version 10.0.26200.1]hi",
            "the banner, and then the command's output and nothing else: {:?}",
            session.events()
        );
        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id }),
            "the markers still did their job: {:?}",
            session.events()
        );
        assert!(
            !session
                .events()
                .contains(&SessionEvent::IntegrationUnavailable),
            "and the session is integrated: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_marker_inside_the_grace_period_keeps_the_session_quiet() {
        let session = Session::with_config(quick_grace()).await;

        session.emit(vec![marker(Osc133Marker::PromptStart)]).await;
        session.advance_to(300).await;

        assert!(
            !session
                .events()
                .contains(&SessionEvent::IntegrationUnavailable),
            "the markers arrived in time: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_late_marker_recovers_a_flagged_session() {
        let session = Session::with_config(quick_grace()).await;
        session.emit(Vec::new()).await;
        session.advance_to(300).await;

        let command_id = session.submit("late").await;
        session
            .emit(command(1, "late", "structured output", Some(2)))
            .await;
        session.advance_to(1_000).await;

        assert_eq!(
            session
                .events()
                .iter()
                .filter(|event| **event == SessionEvent::IntegrationUnavailable)
                .count(),
            1,
            "recovery is silent: {:?}",
            session.events()
        );
        assert_eq!(
            session.started().last(),
            Some(&command_id),
            "the block that opened for the command is the one that was submitted, not a \
             second one — the block before it is the unmarked prompt's, which B6.2 gives a \
             block of its own"
        );
        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id }),
            "the block closed on the markers: {:?}",
            session.events()
        );
        assert!(
            session.announcements().contains(&Announcement::Failed {
                exit_code: ExitCode(2)
            }),
            "and the exit code came back with them, on its only carrier now: {:?}",
            session.announcements()
        );
        assert_eq!(
            session.rendered(),
            "structured output",
            "and so did echo exclusion"
        );
    }

    #[tokio::test]
    async fn an_unintegrated_session_makes_the_echo_the_boundary() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.emit(vec![line(1, "C:\\>")]).await;
        let first = session.submit("one").await;
        session
            .emit(vec![line(1, "one"), line(2, "some output")])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(
            session.output_of(first),
            "some output",
            "the far end's echo of the submitted line is the boundary: the block opens \
             *after* it, so what is under the heading is the command's output and not the \
             command line read back at the user (spec B4.4)"
        );
        assert_eq!(
            session.headings().last(),
            Some(&Some("one".to_owned())),
            "and the heading is the echo the far end produced, not the frontend's guess"
        );
        assert_eq!(
            session.announcements(),
            vec![
                Announcement::ReadAloud {
                    text: "C:\\>".to_owned()
                },
                Announcement::ReadAloud {
                    text: "some output".to_owned()
                }
            ],
            "a session with no integration reads aloud, which is B4.4's whole point — and \
             what it reads is the prompt and then the output, with the echo held on the \
             row it was written onto and dropped when it turned out to be the echo (spec \
             B4.9). Before that, this said `C:\\>one`: the prompt with the user's own \
             command line glued to it, said back at them before the answer: {:?}",
            session.announcements()
        );

        let second = session.submit("two").await;
        session.emit(vec![line(3, "C:\\>"), line(3, "two")]).await;
        session.advance_to(2_000).await;

        assert_eq!(
            session.started().last(),
            Some(&second),
            "the next echo opens the next block: {:?}",
            session.events()
        );
        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id: first }),
            "and closes the one before it: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn a_submission_nothing_echoes_opens_no_block() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.submit("ls").await;
        session.submit("ls").await;
        session.advance_to(2_000).await;

        assert!(
            session.started().is_empty(),
            "no block opens for a line the far end never read: {:?}",
            session.events()
        );
        assert!(
            session.rendered().is_empty(),
            "and there is nothing under it: {:?}",
            session.rendered()
        );
    }

    mod the_echo_is_not_read_back {
        use super::*;

        #[tokio::test]
        async fn no_command_in_an_unintegrated_session_reads_the_typed_line_back() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, PROMPT)]).await;
            let first = session.submit("acter-one").await;
            session
                .emit(vec![line(1, "acter-one"), line(2, "first answer")])
                .await;

            session.emit(vec![line(3, PROMPT)]).await;
            let second = session.submit("acter-two").await;
            session
                .emit(vec![line(3, "acter-two"), line(4, "second answer")])
                .await;
            session.advance_to(2_000).await;

            let said = session.rendered();
            assert!(
                !said.contains("acter-one") && !said.contains("acter-two"),
                "no command line reaches the buffer as text, in any block: {said:?}"
            );
            assert_eq!(
                session.output_of(first),
                format!("first answer\n{PROMPT}"),
                "the first block holds its own answer and the prompt that came back after \
                 it, which is the only ending an unintegrated session has to offer"
            );
            assert_eq!(session.output_of(second), "second answer");
        }

        #[tokio::test]
        async fn a_line_typed_into_a_nested_shell_is_not_read_back() {
            let session = Session::start().await;

            session
                .emit(vec![marker(Osc133Marker::PromptStart), line(1, "> ")])
                .await;
            let enter = session.submit("acter-enter-the-container").await;
            session
                .emit(vec![
                    marker(Osc133Marker::CommandStart),
                    line(1, "acter-enter-the-container"),
                    marker(Osc133Marker::OutputStart),
                    line(2, "/ # "),
                ])
                .await;

            let inside = session.submit("acter-inside").await;
            session
                .emit(vec![line(2, "acter-inside"), line(3, "the answer")])
                .await;
            session.advance_to(1_000).await;

            assert!(
                !session.rendered().contains("acter-inside"),
                "the line typed into the container is not read back: {:?}",
                session.rendered()
            );
            assert!(
                session.output_of(enter).contains("/ # "),
                "the container's prompt still is: {:?}",
                session.output_of(enter)
            );
            assert_eq!(
                session.output_of(inside),
                "the answer",
                "and the block the echo opened holds the answer to it"
            );
        }

        #[tokio::test]
        async fn output_on_any_other_row_is_forwarded_at_once() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("still-pending").await;
            session
                .emit(vec![line(2, "a program is still printing")])
                .await;
            session.advance_to(2_000).await;

            assert!(
                session.rendered().contains("a program is still printing"),
                "a row that is not the pending one is never held: {:?}",
                session.rendered()
            );
        }

        #[tokio::test]
        async fn text_on_the_pending_row_that_is_not_the_echo_is_still_spoken() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("ls").await;
            session
                .emit(vec![line(1, "a password prompt, say, and no echo at all")])
                .await;
            session.advance_to(2_000).await;

            assert!(
                session
                    .rendered()
                    .contains("a password prompt, say, and no echo at all"),
                "past the window it can no longer be an echo, so it is published: {:?}",
                session.rendered()
            );
        }
    }

    mod an_echo_completed_by_a_whole_row_revision {
        use super::*;

        #[tokio::test]
        async fn a_settlement_opens_the_block_and_names_it() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, "/ #")]).await;
            let flood = session.submit("echo one; echo two").await;
            session
                .emit(vec![
                    line(1, " echo one; echo tw"),
                    settled(1, "/ # echo one; echo two"),
                    settled(2, "one"),
                    line(3, "two"),
                ])
                .await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.headings().last(),
                Some(&Some("echo one; echo two".to_owned())),
                "the row the far end wrote is the echo whether it arrived as an append or \
                 as the settlement of a row that scrolled away: {:?}",
                session.events()
            );
            assert_eq!(
                session.output_of(flood),
                "one\ntwo",
                "and the block it opens holds the output that scrolled it away: {:?}",
                session.events()
            );
        }

        #[tokio::test]
        async fn a_rewrite_opens_the_block_and_names_it() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, "/ #")]).await;
            let command = session.submit("ls -la").await;
            session
                .emit(vec![
                    line(1, " ls -l"),
                    rewritten(1, "/ # ls -la"),
                    line(2, "total 0"),
                ])
                .await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.headings().last(),
                Some(&Some("ls -la".to_owned())),
                "{:?}",
                session.events()
            );
            assert_eq!(session.output_of(command), "total 0");
        }

        #[tokio::test]
        async fn a_settlement_on_any_other_row_opens_nothing() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("dir").await;
            session
                .emit(vec![line(1, "dir"), line(2, "one.txt"), line(3, PROMPT)])
                .await;
            session.advance_to(1_000).await;
            let opened = session.started().len();

            session.submit("dir").await;
            session.emit(vec![settled(1, r"C:\>dir")]).await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.started().len(),
                opened,
                "a row the far end wrote before this line was ever submitted is not its \
                 echo, whatever it ends with: {:?}",
                session.events()
            );
        }

        #[tokio::test]
        async fn the_partial_echo_is_not_read_back() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, "/ #")]).await;
            session.submit("echo one; echo two").await;
            session
                .emit(vec![
                    line(1, " echo one; echo tw"),
                    settled(1, "/ # echo one; echo two"),
                    settled(2, "one"),
                ])
                .await;
            session.advance_to(2_000).await;

            assert!(
                !session.rendered().contains("echo tw"),
                "not one character of the command line is read back as output: {:?}",
                session.rendered()
            );
        }

        #[tokio::test]
        async fn what_the_far_end_wrote_in_front_of_the_echo_is_kept() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.submit("echo one").await;
            session
                .emit(vec![
                    line(1, "a banner nobody submitted"),
                    settled(1, "a banner nobody submitted/ # echo one"),
                    settled(2, "one"),
                ])
                .await;
            session.advance_to(2_000).await;

            assert!(
                session.rendered().contains("a banner nobody submitted"),
                "the banner in front of the echo is the far end's and is never dropped: \
                 {:?}",
                session.rendered()
            );
        }
    }

    mod a_bare_enter {
        use super::*;

        #[tokio::test]
        async fn is_written_and_opens_no_block() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, PROMPT)]).await;

            session.submit("").await;
            session.emit(vec![line(2, PROMPT)]).await;
            session.advance_to(2_000).await;

            assert_eq!(session.written(), "\r", "the Enter reaches the far end");
            assert_eq!(
                session.started().len(),
                1,
                "and opens nothing of its own — the one block is the session's own text: \
                 {:?}",
                session.events()
            );
            assert!(
                session.rendered().contains(PROMPT),
                "what the user hears is the prompt coming back: {:?}",
                session.rendered()
            );
        }

        #[tokio::test]
        async fn leaves_nothing_running() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.submit("").await;

            assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
            assert_eq!(session.interrupts(), 0, "and nothing was interrupted");
        }

        #[tokio::test]
        async fn never_takes_the_block_of_the_command_after_it() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, PROMPT)]).await;

            session.submit("").await;
            session.emit(vec![line(2, PROMPT)]).await;
            let command = session.submit("acter-real").await;
            session
                .emit(vec![line(2, "acter-real"), line(3, "its output")])
                .await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.started().last(),
                Some(&command),
                "the block that opened is the command's own: {:?}",
                session.events()
            );
            assert_eq!(session.output_of(command), "its output");
        }
    }

    #[tokio::test]
    async fn a_finished_commands_rows_do_not_settle_into_the_next_block() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.emit(vec![line(9, "C:\\>")]).await;
        session.submit("dir /s").await;
        session
            .emit(vec![
                line(9, "dir /s"),
                line(1, "fms.dll.mui"),
                line(2, "mlang.dll.mui"),
            ])
            .await;
        session.advance_to(1_000).await;

        session.emit(vec![line(10, "C:\\>")]).await;
        let ping = session.submit("ping").await;
        session
            .emit(vec![
                line(10, "ping"),
                line(3, "reply one"),
                settled(1, "fms.dll.mui"),
                line(4, "reply two"),
                settled(2, "mlang.dll.mui"),
            ])
            .await;
        session.advance_to(2_000).await;

        assert_eq!(
            session.output_of(ping),
            "reply one\nreply two",
            "the second block holds its own output and nothing the first one left on \
             screen: {:?}",
            session.outputs()
        );
    }

    #[tokio::test]
    async fn a_row_rewritten_before_the_boundary_does_not_settle_into_the_next_block() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.emit(vec![line(9, "C:\\>")]).await;
        session.submit("first").await;
        session
            .emit(vec![
                line(9, "first"),
                line(1, "downloading"),
                rewritten(1, "downloading done"),
            ])
            .await;
        session.advance_to(1_000).await;

        session.emit(vec![line(10, "C:\\>")]).await;
        let second = session.submit("second").await;
        session
            .emit(vec![
                line(10, "second"),
                line(2, "its own output"),
                settled(1, "downloading done"),
            ])
            .await;
        session.advance_to(2_000).await;

        assert_eq!(
            session.output_of(second),
            "its own output",
            "what the first block was owed stopped being owed when it closed: {:?}",
            session.outputs()
        );
    }

    #[tokio::test]
    async fn a_line_first_seen_settled_inside_the_open_block_is_still_forwarded() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.submit("noisy").await;
        session
            .emit(vec![
                line(1, "still on screen"),
                settled(2, "scrolled past inside one read"),
            ])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(
            session.outputs(),
            vec![
                "still on screen".to_owned(),
                "scrolled past inside one read".to_owned()
            ],
            "dropping a settlement nobody has a record of would lose output: {:?}",
            session.outputs()
        );
    }

    #[tokio::test]
    async fn an_integrated_block_renders_only_its_own_output() {
        let session = Session::start().await;

        session.submit("first").await;
        session
            .emit(command(1, "first", "first output", Some(0)))
            .await;
        session.submit("second").await;
        session
            .emit(command(3, "second", "second output", Some(0)))
            .await;
        session.advance_to(2_000).await;

        assert_eq!(
            session.outputs(),
            vec!["first output".to_owned(), "second output".to_owned()],
            "echo exclusion still decides what a marked block contains: {:?}",
            session.outputs()
        );
    }

    #[tokio::test]
    async fn a_line_submitted_during_the_grace_period_opens_when_it_is_echoed() {
        let session = Session::with_config(quick_grace()).await;

        let command_id = session.submit("early").await;
        session.advance_to(300).await;
        session
            .emit(vec![line(1, "early"), line(2, "output after the flag")])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(
            session.started().last(),
            Some(&command_id),
            "the submission opens its block when its echo arrives: {:?}",
            session.events()
        );
        assert_eq!(session.output_of(command_id), "output after the flag");
    }

    #[tokio::test]
    async fn an_interrupt_does_not_close_the_command_in_an_unintegrated_session() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.submit("forever").await;
        session.emit(vec![line(1, "still working")]).await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);
        assert_eq!(session.interrupts(), 1);
        assert!(
            !session
                .events()
                .iter()
                .any(|event| matches!(event, SessionEvent::CommandInterrupted { .. })),
            "the interrupt is not a boundary: {:?}",
            session.events()
        );
        assert_eq!(
            session.press(ctrl('c')).await,
            KeyAck::Applied,
            "and the command is still running, so there is still something to stop"
        );
    }

    #[tokio::test]
    async fn an_interrupt_does_not_close_the_command_in_an_integrated_session() {
        let session = Session::start().await;

        session.submit("forever").await;
        session
            .emit(vec![marker(Osc133Marker::OutputStart), line(1, "working")])
            .await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);
        session.advance_to(1_000).await;

        assert!(
            !session
                .events()
                .iter()
                .any(|event| matches!(event, SessionEvent::CommandInterrupted { .. })),
            "nothing closed until the block ends: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn output_arriving_after_an_interrupt_still_reaches_the_frontend() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.submit("forever").await;
        session.emit(vec![line(1, "still working")]).await;
        session.press(ctrl('c')).await;

        session.emit(vec![line(2, r"C:\>")]).await;
        session.advance_to(1_000).await;

        assert!(
            session.rendered().contains(r"C:\>"),
            "the prompt that came back is what the user hears: {:?}",
            session.rendered()
        );
    }

    #[tokio::test]
    async fn the_next_commands_echo_closes_an_interrupted_command_as_stopped() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.emit(vec![line(1, "C:\\>")]).await;
        let command_id = session.submit("forever").await;
        session
            .emit(vec![line(1, "forever"), line(2, "still working")])
            .await;
        session.press(ctrl('c')).await;

        session.submit("next").await;
        session.emit(vec![line(3, "C:\\>"), line(3, "next")]).await;
        session.advance_to(2_000).await;

        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandInterrupted { command_id }),
            "{:?}",
            session.events()
        );
        assert!(
            !session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id }),
            "and never also as finished: {:?}",
            session.events()
        );
    }

    #[tokio::test]
    async fn re_attaching_moves_the_events_to_the_new_sink() {
        let session = Session::start().await;
        let reloaded = Arc::new(Recorder::default());
        session
            .api
            .attach_session(SessionId(1), Arc::clone(&reloaded) as Arc<dyn EventSink>);

        session.submit("after the reload").await;
        session
            .emit(command(1, "after the reload", "output", Some(0)))
            .await;
        session.advance_to(1_000).await;

        assert!(
            session.events().is_empty(),
            "the old channel is gone: {:?}",
            session.events()
        );
        assert!(
            !reloaded.0.lock().expect("recorder poisoned").is_empty(),
            "and the new one has the session"
        );
    }

    mod the_prompt_a_marked_shell_draws {
        use super::*;

        async fn marked() -> Session {
            Session::of(quick_grace(), ShellMarkers::Full).await
        }

        fn prompt(row: u64, at: &str) -> Vec<TerminalItem> {
            vec![
                marker(Osc133Marker::PromptStart),
                line(row, at),
                marker(Osc133Marker::CommandStart),
            ]
        }

        fn prompts(session: &Session) -> Vec<String> {
            session
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::PromptDrawn { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn is_spoken_before_any_command_has_run() {
            let session = marked().await;
            session.emit(prompt(1, PROMPT)).await;
            session.advance_to(1_000).await;

            assert_eq!(prompts(&session), vec![PROMPT.to_owned()]);
        }

        #[tokio::test]
        async fn is_spoken_when_its_end_arrives_in_a_read_of_its_own() {
            let session = marked().await;
            session.emit(vec![marker(Osc133Marker::PromptStart)]).await;
            session.emit(vec![line(1, PROMPT)]).await;
            session.emit(vec![marker(Osc133Marker::CommandStart)]).await;
            session.advance_to(1_000).await;

            assert_eq!(
                prompts(&session),
                vec![PROMPT.to_owned()],
                "the prompt is finished when B arrives, not when the next command is typed"
            );
        }

        #[tokio::test]
        async fn is_spoken_again_after_every_command() {
            let session = marked().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("cd project").await;
            session
                .emit(vec![marker(Osc133Marker::OutputStart), line(2, "done")])
                .await;
            session
                .emit(vec![marker(Osc133Marker::CommandEnd(Some(ExitCode(0))))])
                .await;
            session.emit(prompt(3, r"C:\project>")).await;
            session.advance_to(1_000).await;

            assert_eq!(
                prompts(&session),
                vec![PROMPT.to_owned(), r"C:\project>".to_owned()],
                "both prompts, and the second says where the command left the user"
            );
            assert!(
                !session.output_of(command).contains(r"C:\project>"),
                "and it is not block content: {:?}",
                session.output_of(command)
            );
        }

        #[tokio::test]
        async fn the_same_prompt_after_a_command_is_still_announced() {
            let session = marked().await;
            session.emit(prompt(1, PROMPT)).await;
            let _ = session.submit("git status").await;
            session
                .emit(vec![marker(Osc133Marker::OutputStart), line(2, "clean")])
                .await;
            session
                .emit(vec![marker(Osc133Marker::CommandEnd(Some(ExitCode(0))))])
                .await;
            session.emit(prompt(3, PROMPT)).await;
            session.advance_to(1_000).await;

            assert_eq!(
                prompts(&session),
                vec![PROMPT.to_owned(), PROMPT.to_owned()],
                "a command ran, so the prompt coming back is news however it reads"
            );
        }

        #[tokio::test]
        async fn arrives_after_the_command_it_follows_has_finished() {
            let session = marked().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("dir").await;
            session
                .emit(vec![marker(Osc133Marker::OutputStart), line(2, "one.txt")])
                .await;
            session
                .emit(vec![marker(Osc133Marker::CommandEnd(Some(ExitCode(0))))])
                .await;
            session.emit(prompt(3, PROMPT)).await;
            session.advance_to(1_000).await;

            let events = session.events();
            let finished = events
                .iter()
                .position(|event| matches!(event, SessionEvent::CommandFinished { .. }))
                .expect("the command finished");
            let spoken = events
                .iter()
                .rposition(|event| matches!(event, SessionEvent::PromptDrawn { .. }))
                .expect("the prompt was drawn");

            assert!(
                finished < spoken,
                "the verdict comes before the new prompt, and the events were {events:?}"
            );
        }

        #[tokio::test]
        async fn an_empty_prompt_is_not_announced() {
            let session = marked().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, "   "),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.advance_to(1_000).await;

            assert!(prompts(&session).is_empty());
        }
    }

    mod a_shell_that_marks_no_output_start {
        use super::*;

        async fn cmd() -> Session {
            Session::of(quick_grace(), ShellMarkers::PromptAndCommandLine).await
        }

        fn prompt(row: u64, at: &str) -> Vec<TerminalItem> {
            vec![
                marker(Osc133Marker::PromptStart),
                line(row, at),
                marker(Osc133Marker::CommandStart),
            ]
        }

        #[tokio::test]
        async fn the_echo_opens_the_submissions_block_and_names_it() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("dir").await;
            session.emit(vec![line(1, "dir"), line(2, "one.txt")]).await;
            session.advance_to(1_000).await;

            assert_eq!(session.started().last(), Some(&command));
            assert_eq!(session.headings(), vec![None, Some("dir".to_owned())]);
            assert_eq!(session.output_of(command), "one.txt");
        }

        #[tokio::test]
        async fn the_prompt_comes_back_inside_the_block_it_ended() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("dir").await;
            session.emit(vec![line(1, "dir"), line(2, "one.txt")]).await;
            session.emit(prompt(3, PROMPT)).await;
            session.advance_to(1_000).await;

            assert!(
                session.output_of(command).contains(PROMPT),
                "the prompt is the last thing the block says, and it was {:?}",
                session.output_of(command)
            );
            assert!(
                session.events().iter().any(|event| matches!(
                    event,
                    SessionEvent::CommandFinished { command_id } if *command_id == command
                )),
                "the block closes"
            );
        }

        #[tokio::test]
        async fn the_echo_is_never_the_blocks_content() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("dir").await;
            session.emit(vec![line(1, "dir"), line(2, "one.txt")]).await;
            session.advance_to(1_000).await;

            assert!(!session.output_of(command).contains("dir"));
        }

        #[tokio::test]
        async fn text_that_is_not_the_echo_is_never_dropped() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("dir").await;
            session
                .emit(vec![line(7, "something the far end wrote")])
                .await;
            session.advance_to(1_000).await;

            assert!(session.rendered().contains("something the far end wrote"));
        }
    }

    mod a_shell_that_marks_no_output_start_but_says_how_the_command_went {
        use super::*;

        async fn posix_sh() -> Session {
            Session::of(quick_grace(), ShellMarkers::PromptCommandLineAndExitCode).await
        }

        fn prompt(row: u64, at: &str) -> Vec<TerminalItem> {
            vec![
                marker(Osc133Marker::PromptStart),
                line(row, at),
                marker(Osc133Marker::CommandStart),
            ]
        }

        fn prompts(session: &Session) -> Vec<String> {
            session
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::PromptDrawn { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn a_command_that_fails_is_announced_as_having_failed() {
            let session = posix_sh().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("(exit 7)").await;
            session
                .emit(vec![
                    line(1, "(exit 7)"),
                    line(2, "output"),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(7)))),
                ])
                .await;
            session.advance_to(1_000).await;

            assert!(
                session.announcements().contains(&Announcement::Failed {
                    exit_code: ExitCode(7)
                }),
                "the verdict a shell with only a `PS1` was believed unable to give: {:?}",
                session.announcements()
            );
            assert!(
                session.events().iter().any(|event| matches!(
                    event,
                    SessionEvent::CommandFinished { command_id } if *command_id == command
                )),
                "and the block closes on it"
            );
        }

        #[tokio::test]
        async fn the_prompt_is_announced_on_its_own_rather_than_as_block_content() {
            let session = posix_sh().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("ls").await;
            session
                .emit(vec![
                    line(1, "ls"),
                    line(2, "one.txt"),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;
            session.emit(prompt(3, PROMPT)).await;
            session.advance_to(1_000).await;

            assert_eq!(
                session.output_of(command),
                "one.txt",
                "the returning prompt is not what the command printed"
            );
            assert!(
                prompts(&session).contains(&PROMPT.to_owned()),
                "and a listener hears it as the prompt: {:?}",
                prompts(&session)
            );
        }

        #[tokio::test]
        async fn the_echo_still_opens_the_block_and_is_never_its_content() {
            let session = posix_sh().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("ls").await;
            session.emit(vec![line(1, "ls"), line(2, "one.txt")]).await;
            session.advance_to(1_000).await;

            assert_eq!(session.started().last(), Some(&command));
            assert_eq!(session.headings().last(), Some(&Some("ls".to_owned())));
            assert_eq!(session.output_of(command), "one.txt");
        }
    }

    mod the_cancel_ahead_of_a_submission {
        use super::*;

        async fn cmd() -> Session {
            Session::over(quick_grace(), discarding_on(0x1b)).await
        }

        fn prompt(row: u64, at: &str) -> Vec<TerminalItem> {
            vec![
                marker(Osc133Marker::PromptStart),
                line(row, at),
                marker(Osc133Marker::CommandStart),
            ]
        }

        #[tokio::test]
        async fn a_shell_at_its_prompt_gets_one() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("dir").await;

            assert_eq!(
                session.written(),
                "\u{1b}dir\r",
                "the cancel goes out on its own, then the line"
            );
        }

        #[tokio::test]
        async fn a_bare_enter_at_the_prompt_gets_one() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("").await;

            assert_eq!(
                session.written(),
                "\u{1b}\r",
                "the cancel, and then the Enter it protects"
            );
        }

        #[tokio::test]
        async fn a_bare_enter_into_a_running_program_gets_none() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("python").await;
            session.emit(vec![line(1, "python"), line(2, ">>>")]).await;
            session.submit("").await;

            assert_eq!(
                session.written(),
                "\u{1b}python\r\r",
                "the Enter goes out on its own — only the first submission, made at the \
                 prompt, carried a cancel"
            );
        }

        #[tokio::test]
        async fn a_running_command_gets_none() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("python").await;
            session.emit(vec![line(1, "python"), line(2, ">>>")]).await;
            session.submit("2 + 2").await;

            assert!(
                session.written().ends_with("2 + 2\r"),
                "and nothing in front of it: {:?}",
                session.written()
            );
            assert_eq!(
                session.written().matches('\u{1b}').count(),
                1,
                "only the first submission, made at the prompt, carried one: {:?}",
                session.written()
            );
        }

        #[tokio::test]
        async fn a_line_submitted_behind_another_gets_none() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("first").await;
            session.submit("second").await;

            assert_eq!(
                session.written().matches('\u{1b}').count(),
                1,
                "one cancel, for the line that was actually at the prompt: {:?}",
                session.written()
            );
        }

        #[tokio::test]
        async fn a_shell_that_named_no_such_byte_never_gets_one() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("dir").await;

            assert_eq!(session.written(), "dir\r");
        }

        #[tokio::test]
        async fn a_shell_that_marks_only_its_prompt_gets_none_unless_it_asked_for_one() {
            let session = Session::of(quick_grace(), ShellMarkers::PromptAndCommandLine).await;
            session.emit(prompt(1, PROMPT)).await;
            session.submit("dir").await;

            assert_eq!(
                session.written(),
                "dir\r",
                "marking only the prompt is not the same fact as discarding on a byte"
            );
        }
    }

    mod the_far_end_owns_the_line {
        use super::*;

        fn named(key: Key) -> KeyPress {
            KeyPress {
                key,
                ctrl: false,
                shift: false,
                alt: false,
            }
        }

        async fn at_a_prompt() -> Session {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, "user@host:~$ ")]).await;
            session.cursor_at(13, 0).await;
            session.advance_to(1_000).await;
            session
        }

        #[tokio::test]
        async fn taking_the_line_hands_over_the_row_from_the_anchor() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            assert_eq!(
                session.far_end_lines(),
                vec![(Some(String::new()), 0)],
                "the anchor is taken where the far end's cursor came to rest"
            );
        }

        #[tokio::test]
        async fn typing_extends_the_line_without_moving_the_anchor() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            let _ = session.press(named(Key::Char('l'))).await;
            let _ = session.press(named(Key::Char('s'))).await;
            session.emit(vec![line(1, "ls")]).await;
            session.cursor_at(15, 0).await;
            session.advance_to(4_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls".to_owned()), 2)),
                "the prompt stays behind and the command line is what is handed over"
            );
        }

        #[tokio::test]
        async fn a_trailing_space_is_in_the_field_and_deleting_it_is_a_change() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            let _ = session.press(named(Key::Char('l'))).await;
            let _ = session.press(named(Key::Char('s'))).await;
            session.emit(vec![line(1, "ls")]).await;
            session.cursor_at(15, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Char(' '))).await;
            session.emit(vec![]).await;
            session.cursor_at(16, 0).await;
            session.advance_to(7_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls ".to_owned()), 3)),
                "the cursor is past the row's last character, so the space is there"
            );

            assert_eq!(session.press(named(Key::Backspace)).await, KeyAck::Applied);
            session.emit(vec![]).await;
            session.cursor_at(15, 0).await;
            session.advance_to(10_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls".to_owned()), 2)),
                "the line the listener holds is a character shorter, which is a change"
            );
        }

        #[tokio::test]
        async fn up_hands_over_the_recalled_line_without_the_prompt() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            assert_eq!(session.press(named(Key::Up)).await, KeyAck::Applied);
            assert_eq!(session.written(), "\x1b[A", "the measured spelling");

            session.emit(vec![rewritten(1, "user@host:~$ exit")]).await;
            session.cursor_at(17, 0).await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("exit".to_owned()), 4)),
                "the anchor is what keeps the prompt out of it"
            );
        }

        #[tokio::test]
        async fn the_first_recall_appends_and_is_still_the_answer() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Up)).await;

            session.emit(vec![line(1, "echo one")]).await;
            session.cursor_at(21, 0).await;
            session.advance_to(2_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("echo one".to_owned()), 8))
            );
        }

        #[tokio::test]
        async fn a_key_that_moves_only_the_cursor_moves_only_the_caret() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(1, "ls -la")]).await;
            session.cursor_at(19, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Left)).await;
            session.cursor_at(18, 0).await;
            session.emit(vec![]).await;
            session.advance_to(8_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(None, 5)),
                "no text, because no text changed"
            );
        }

        #[tokio::test]
        async fn a_menu_repaint_answers_with_the_command_line() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('G'))).await;
            session.emit(vec![line(1, "Get-C")]).await;
            session.cursor_at(18, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Down)).await;
            let mut repaint = vec![rewritten(1, "user@host:~$ Get-Command")];
            for row in 2..12 {
                repaint.push(rewritten(row, ""));
            }
            session.emit(repaint).await;
            session.cursor_at(24, 0).await;
            session.advance_to(8_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("Get-Command".to_owned()), 11)),
                "row count routes nothing: this is ordinary Tab completion"
            );
        }

        #[tokio::test]
        async fn a_selection_prompt_answers_with_the_row_that_gained_content() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            session.hides_its_cursor().await;
            session
                .emit(vec![
                    line(2, "> marlon-sousa/acter"),
                    line(3, "  Skip pushing the branch"),
                ])
                .await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Down)).await;
            session
                .emit(vec![
                    rewritten(2, "  marlon-sousa/acter"),
                    rewritten(3, "> Skip pushing the branch"),
                ])
                .await;
            session.advance_to(8_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("> Skip pushing the branch".to_owned()), 0)),
                "one option per press, and not the one they just left"
            );
        }

        #[tokio::test]
        async fn a_widget_that_hides_its_cursor_still_gets_the_content_rule() {
            let session = at_a_prompt().await;
            session.hides_its_cursor().await;
            session
                .emit(vec![
                    line(2, "> marlon-sousa/acter"),
                    line(3, "  Skip pushing the branch"),
                ])
                .await;
            session.advance_to(2_000).await;

            session.owner(LineOwner::FarEnd).await;
            assert_eq!(
                session.far_end_lines(),
                Vec::new(),
                "nothing is anchored to a cursor the far end is not showing"
            );

            let _ = session.press(named(Key::Down)).await;
            session
                .emit(vec![
                    rewritten(2, "  marlon-sousa/acter"),
                    rewritten(3, "> Skip pushing the branch"),
                ])
                .await;
            session.advance_to(4_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("> Skip pushing the branch".to_owned()), 0)),
                "the row that gained content is the answer, anchor or no anchor"
            );
        }

        #[tokio::test]
        async fn a_keystroke_is_answered_while_the_reader_is_still_listening() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let answered = session.far_end_lines().len();

            let _ = session.press(named(Key::Up)).await;
            session
                .emit(vec![rewritten(1, "user@host:~$ echo one")])
                .await;
            session.cursor_at(21, 0).await;
            session.advance_to(1_030).await;

            assert_eq!(
                session.far_end_lines().len(),
                answered + 1,
                "the recalled line is in the field before the caret poll gives up"
            );
            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("echo one".to_owned()), 8)),
                "and it is the recalled line, with the caret at its end"
            );
        }

        #[tokio::test]
        async fn a_redraw_that_arrives_in_pieces_is_coalesced_into_one_answer() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            session.hides_its_cursor().await;
            session
                .emit(vec![
                    line(2, "> Create a new repository"),
                    line(3, "  Push an existing repository"),
                ])
                .await;
            session.advance_to(1_500).await;
            let answered = session.far_end_lines().len();

            let _ = session.press(named(Key::Down)).await;
            session
                .emit(vec![rewritten(2, "  Create a new repository")])
                .await;
            session.advance_to(1_510).await;
            session
                .emit(vec![rewritten(3, "> Push an existing repository")])
                .await;
            session.advance_to(1_545).await;

            assert_eq!(
                session.far_end_lines().len(),
                answered + 1,
                "one press, one answer, however many writes the far end took"
            );
            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("> Push an existing repository".to_owned()), 0)),
                "and it is the option they moved to, not the row that was erased"
            );
        }

        #[tokio::test]
        async fn output_nobody_pressed_a_key_for_keeps_the_pacing_clock() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let anchored = session.far_end_lines().len();

            session.emit(vec![line(2, "some output")]).await;
            session.advance_to(1_100).await;

            assert_eq!(
                session.far_end_lines().len(),
                anchored,
                "a keystroke's clock is not a transcript's: nothing re-anchored at 100ms"
            );

            session.advance_to(1_600).await;
            assert_eq!(
                session.far_end_lines().len(),
                anchored + 1,
                "and the anchor is still taken, on the clock that has always taken it"
            );
        }

        #[tokio::test]
        async fn an_anchor_is_never_taken_from_a_prompt_still_being_drawn() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(1, "ls")]).await;
            session.cursor_at(15, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Enter)).await;
            session.emit(vec![line(4, "user@host:~$ ls")]).await;
            session.cursor_at(0, 1).await;
            session.advance_to(4_100).await;

            session.hides_its_cursor().await;
            session
                .emit(vec![line(5, "? What would you like to do?")])
                .await;
            session.advance_to(6_000).await;

            let _ = session.press(named(Key::Enter)).await;

            assert_eq!(
                session.headings().into_iter().flatten().collect::<Vec<_>>(),
                vec!["ls".to_owned()],
                "answering a prompt is not running a command, and heads no block"
            );
        }

        #[tokio::test]
        async fn a_command_line_redrawn_on_another_row_is_followed_there() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(1, "ls /tmp/al")]).await;
            session.cursor_at(23, 0).await;
            session.advance_to(4_000).await;
            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls /tmp/al".to_owned()), 10)),
                "the line being edited, before any of this"
            );

            let _ = session.press(named(Key::Tab)).await;
            session
                .emit(vec![
                    line(4, "alpha-one.txt  alpha-two.txt"),
                    line(5, "user@host:~$ ls /tmp/al"),
                ])
                .await;
            session.cursor_at(23, 2).await;
            session.advance_to(4_100).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls /tmp/al".to_owned()), 10)),
                "the field holds the line being edited, not the candidates"
            );

            let _ = session.press(named(Key::Backspace)).await;
            session
                .emit(vec![rewritten(5, "user@host:~$ ls /tmp/a")])
                .await;
            session.cursor_at(22, 2).await;
            session.advance_to(4_200).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls /tmp/a".to_owned()), 9)),
                "one character shorter, still without the prompt"
            );
        }

        #[tokio::test]
        async fn what_the_far_end_printed_at_its_prompt_reaches_the_transcript() {
            let session = Session::start().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(0, "user@host:~$ "),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.cursor_at(13, 0).await;
            session.advance_to(1_000).await;
            session.owner(LineOwner::FarEnd).await;

            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(0, "ls /tmp/al")]).await;
            session.cursor_at(23, 0).await;
            session.advance_to(2_000).await;

            let _ = session.press(named(Key::Tab)).await;
            session
                .emit(vec![
                    line(4, "alpha-one.txt  alpha-two.txt"),
                    line(5, "user@host:~$ ls /tmp/al"),
                ])
                .await;
            session.cursor_at(23, 2).await;
            session.advance_to(2_100).await;

            let rendered = session.rendered();
            assert!(
                rendered.contains("alpha-one.txt  alpha-two.txt"),
                "the candidates are in the transcript rather than lost: {rendered:?}"
            );
            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls /tmp/al".to_owned()), 10)),
                "and the field still holds the line being edited"
            );
        }

        #[tokio::test]
        async fn the_settling_after_a_submission_takes_the_next_anchor() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(1, "ls")]).await;
            session.cursor_at(15, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Enter)).await;
            session.emit(vec![line(4, "user@host:~$ ")]).await;
            session.cursor_at(13, 1).await;
            session.advance_to(8_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some(String::new()), 0)),
                "the new command line is empty, and it is the one the field now holds"
            );

            let _ = session.press(named(Key::Up)).await;
            session.emit(vec![rewritten(4, "user@host:~$ ls")]).await;
            session.cursor_at(15, 1).await;
            session.advance_to(12_000).await;

            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("ls".to_owned()), 2))
            );
        }

        #[tokio::test]
        async fn enter_opens_a_block_headed_by_the_row_the_far_end_echoed() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let _ = session.press(named(Key::Char('c'))).await;
            session.emit(vec![line(1, "cargo test")]).await;
            session.cursor_at(23, 0).await;
            session.advance_to(4_000).await;

            let _ = session.press(named(Key::Enter)).await;

            assert!(
                session.written().ends_with('\r'),
                "Enter is a carriage return: {:?}",
                session.written()
            );
            assert_eq!(
                session.headings().last(),
                Some(&Some("cargo test".to_owned())),
                "the anchored row is the heading"
            );
        }

        #[tokio::test]
        async fn enter_on_an_empty_row_earns_no_block_and_no_heading() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let before = session.started().len();

            let _ = session.press(named(Key::Enter)).await;

            assert_eq!(
                session.started().len(),
                before,
                "nothing was typed, so nothing was echoed, so nothing ran"
            );
        }

        #[tokio::test]
        async fn the_anchored_row_is_rendered_and_never_spoken() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            let spoken_before = session.announcements().len();

            let _ = session.press(named(Key::Char('l'))).await;
            session.emit(vec![line(1, "l")]).await;
            session.advance_to(4_000).await;

            assert!(
                session.rendered().contains('l'),
                "the far end drew it, so the buffer keeps it: {:?}",
                session.rendered()
            );
            assert_eq!(
                session.announcements().len(),
                spoken_before,
                "and the reader speaks the field rather than Acter speaking the row"
            );
        }

        #[tokio::test]
        async fn ctrl_c_reaches_the_far_end_as_a_byte_rather_than_an_interrupt() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);

            assert!(
                session.written().ends_with('\u{3}'),
                "the control byte, not a transport interrupt: {:?}",
                session.written()
            );
            assert_eq!(
                session.interrupts(),
                0,
                "nothing asked the transport to interrupt anything"
            );
        }

        #[tokio::test]
        async fn application_cursor_keys_change_what_an_arrow_costs() {
            let session = at_a_prompt().await;
            session
                .asks_for(TerminalModes {
                    application_cursor_keys: true,
                    bracketed_paste: false,
                })
                .await;
            session.owner(LineOwner::FarEnd).await;

            let _ = session.press(named(Key::Up)).await;

            assert!(
                session.written().ends_with("\x1bOA"),
                "the far end asked for the application form: {:?}",
                session.written()
            );
        }

        #[tokio::test]
        async fn a_paste_is_bracketed_only_when_the_far_end_asked_for_it() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;

            session.api.paste(SessionId(1), "one\ntwo");
            session.settle().await;
            assert!(
                session.written().ends_with("one\ntwo"),
                "bare, because nothing asked: {:?}",
                session.written()
            );

            session
                .asks_for(TerminalModes {
                    application_cursor_keys: false,
                    bracketed_paste: true,
                })
                .await;
            session.api.paste(SessionId(1), "three");
            session.settle().await;
            assert!(
                session.written().ends_with("\x1b[200~three\x1b[201~"),
                "wrapped, because the far end asked: {:?}",
                session.written()
            );
        }

        #[tokio::test]
        async fn taking_the_line_back_puts_the_keys_where_they_were() {
            let session = at_a_prompt().await;
            session.owner(LineOwner::FarEnd).await;
            session.owner(LineOwner::Local).await;
            let written = session.written();

            assert_eq!(session.press(ctrl('c')).await, KeyAck::Applied);

            assert_eq!(
                session.interrupts(),
                1,
                "the transport was asked, rather than a control byte written"
            );
            assert_eq!(
                session.written(),
                written,
                "and nothing went down the wire for it"
            );
        }

        #[tokio::test]
        async fn a_named_key_is_unbound_while_acter_owns_the_line() {
            let session = at_a_prompt().await;

            assert_eq!(session.press(named(Key::Up)).await, KeyAck::Unbound);
            assert_eq!(session.written(), "");
        }
    }

    mod completing_at_an_integrated_far_end {
        use super::*;

        fn named(key: Key) -> KeyPress {
            KeyPress {
                key,
                ctrl: false,
                shift: false,
                alt: false,
            }
        }

        fn spoken(session: &Session) -> String {
            session
                .announcements()
                .into_iter()
                .filter_map(|said| match said {
                    Announcement::ReadAloud { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        fn prompts(session: &Session) -> Vec<String> {
            session
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::PromptDrawn { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        async fn about_to_complete() -> Session {
            let session = Session::start().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, "bash-5.2$"),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.cursor_at(10, 0).await;
            session.advance_to(1_000).await;
            session.owner(LineOwner::FarEnd).await;

            for key in ['c', 'd', ' ', 'a'] {
                let _ = session.press(named(Key::Char(key))).await;
            }
            session.emit(vec![line(1, " cd a")]).await;
            session.cursor_at(14, 0).await;
            session.advance_to(2_000).await;
            session
        }

        async fn two_tabs(session: &Session) {
            let _ = session.press(named(Key::Tab)).await;
            session.emit(vec![]).await;
            session.advance_to(3_000).await;

            let _ = session.press(named(Key::Tab)).await;
            session
                .emit(vec![
                    line(2, "alpha/ axel/"),
                    marker(Osc133Marker::PromptStart),
                    line(3, "bash-5.2$"),
                    marker(Osc133Marker::CommandStart),
                    line(3, " cd a"),
                ])
                .await;
            session.cursor_at(14, 2).await;
            session.advance_to(4_000).await;
            // A second tick: the pump settling arms the actor, which reads out on its own clock.
            session.advance_to(4_600).await;
        }

        #[tokio::test]
        async fn a_completion_redraw_does_not_announce_the_prompt_again() {
            let session = about_to_complete().await;
            two_tabs(&session).await;

            assert_eq!(
                prompts(&session),
                vec!["bash-5.2$".to_owned()],
                "the prompt drawn once and repainted once is one prompt: {:?}",
                session.events()
            );
        }

        #[tokio::test]
        async fn the_candidates_reach_the_transcript_and_are_read_aloud() {
            let session = about_to_complete().await;
            two_tabs(&session).await;

            assert!(
                session.rendered().contains("alpha/ axel/"),
                "the candidates are in the transcript: {:?}",
                session.rendered()
            );
            assert!(
                spoken(&session).contains("alpha/ axel/"),
                "and they are read aloud on the ordinary pacing path: {:?}",
                session.announcements()
            );
            assert_eq!(
                session.far_end_lines().last(),
                Some(&(Some("cd a".to_owned()), 4)),
                "while the field still holds the line being edited"
            );
        }
    }

    mod an_empty_enter_at_an_integrated_shell {
        use super::*;

        fn failures(session: &Session) -> Vec<ExitCode> {
            session
                .announcements()
                .into_iter()
                .filter_map(|said| match said {
                    Announcement::Failed { exit_code } => Some(exit_code),
                    _ => None,
                })
                .collect()
        }

        fn prompts(session: &Session) -> Vec<String> {
            session
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::PromptDrawn { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        async fn after_a_failure() -> Session {
            let session = Session::start().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, PROMPT),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.submit("false").await;
            session
                .emit(vec![
                    line(1, "false"),
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(1)))),
                    marker(Osc133Marker::PromptStart),
                    line(2, PROMPT),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.advance_to(1_000).await;
            session
        }

        async fn an_empty_enter(session: &Session, row: u64, at: u64) {
            session.submit("").await;
            session
                .emit(vec![
                    marker(Osc133Marker::OutputStart),
                    settled(row, PROMPT),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(1)))),
                    marker(Osc133Marker::PromptStart),
                    line(row + 1, PROMPT),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.advance_to(at).await;
        }

        #[tokio::test]
        async fn does_not_repeat_the_verdict_of_the_command_before_it() {
            let session = after_a_failure().await;
            an_empty_enter(&session, 2, 2_000).await;

            assert_eq!(
                failures(&session),
                vec![ExitCode(1)],
                "the failure is announced once, by the command that failed: {:?}",
                session.announcements()
            );
        }

        #[tokio::test]
        async fn nor_at_the_third_press_of_enter() {
            let session = after_a_failure().await;
            an_empty_enter(&session, 2, 2_000).await;
            an_empty_enter(&session, 3, 3_000).await;
            an_empty_enter(&session, 4, 4_000).await;

            assert_eq!(failures(&session), vec![ExitCode(1)]);
        }

        #[tokio::test]
        async fn the_block_it_opens_is_closed_all_the_same() {
            let session = after_a_failure().await;
            an_empty_enter(&session, 2, 2_000).await;

            let opened = *session.started().last().expect("a block opened");
            assert!(
                session.events().iter().any(|event| matches!(
                    event,
                    SessionEvent::CommandFinished { command_id } if *command_id == opened
                )),
                "the block the empty Enter opened ends: {:?}",
                session.events()
            );
            assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
        }

        #[tokio::test]
        async fn the_prompt_that_comes_back_is_still_spoken() {
            let session = after_a_failure().await;
            let before = prompts(&session).len();
            an_empty_enter(&session, 2, 2_000).await;

            assert_eq!(
                prompts(&session).len(),
                before + 1,
                "the prompt after the empty Enter: {:?}",
                prompts(&session)
            );
        }

        #[tokio::test]
        async fn a_command_that_fails_silently_and_unrecognised_keeps_its_verdict() {
            let session = Session::start().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, PROMPT),
                    marker(Osc133Marker::CommandStart),
                ])
                .await;
            session.submit("false").await;
            session
                .emit(vec![
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(1)))),
                ])
                .await;
            session.advance_to(1_000).await;

            assert_eq!(
                failures(&session),
                vec![ExitCode(1)],
                "no echo, no output, and still a verdict: {:?}",
                session.announcements()
            );
        }

        #[tokio::test]
        async fn a_block_nobody_submitted_that_printed_something_keeps_its_verdict() {
            let session = Session::start().await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, PROMPT),
                    marker(Osc133Marker::CommandStart),
                    marker(Osc133Marker::OutputStart),
                    line(2, "a far end talking to itself"),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(2)))),
                ])
                .await;
            session.advance_to(1_000).await;

            assert!(
                session.rendered().contains("a far end talking to itself"),
                "the text reached the buffer: {:?}",
                session.rendered()
            );
            assert_eq!(
                failures(&session),
                vec![ExitCode(2)],
                "and the verdict on it is announced: {:?}",
                session.announcements()
            );
        }
    }

    mod end_of_input {
        use super::*;

        #[tokio::test]
        async fn the_session_writes_whatever_the_shell_said_ends_it() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;

            assert_eq!(session.press(ctrl('d')).await, KeyAck::Applied);
            assert_eq!(session.written(), "stop-this-shell");
        }

        #[tokio::test]
        async fn a_shell_with_no_answer_writes_nothing_and_says_so() {
            let session = Session::with_config(quick_grace()).await;

            assert_eq!(session.press(ctrl('d')).await, KeyAck::Unsupported);
            assert_eq!(session.written(), "");
        }

        #[tokio::test]
        async fn nothing_needs_to_be_running_for_it_to_apply() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;

            assert_eq!(
                session.press(ctrl('c')).await,
                KeyAck::NothingToActOn,
                "the interrupt has nothing to stop"
            );
            assert_eq!(
                session.press(ctrl('d')).await,
                KeyAck::Applied,
                "and ending the session does not need one"
            );
        }

        #[tokio::test]
        async fn it_is_not_a_submission_and_gets_no_block() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;
            session.press(ctrl('d')).await;

            assert!(
                session.started().is_empty(),
                "no command started: {:?}",
                session.events()
            );
        }

        #[tokio::test]
        async fn it_is_not_an_interrupt() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;
            session.press(ctrl('d')).await;

            assert_eq!(session.interrupts(), 0);
        }
    }

    mod the_setup_is_sent_once_the_far_end_speaks {
        use super::*;

        const SETUP: &str = "printf mark; PROMPT_COMMAND=__acter_prompt";

        async fn set_up() -> Session {
            Session::over(quick_grace(), set_up_with(SETUP)).await
        }

        #[tokio::test]
        async fn nothing_is_written_before_the_far_end_has_said_anything() {
            let session = set_up().await;

            assert_eq!(session.written(), "");
        }

        #[tokio::test]
        async fn the_setup_goes_out_on_the_far_ends_first_byte() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;

            assert_eq!(
                session.written(),
                format!("{SETUP}\r"),
                "submitted exactly as a typed line is, Enter included"
            );
        }

        #[tokio::test]
        async fn it_is_sent_once_however_often_the_far_end_speaks() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.emit(vec![line(2, "more")]).await;
            session.emit(vec![line(3, "and more")]).await;

            assert_eq!(session.written(), format!("{SETUP}\r"));
        }

        #[tokio::test]
        async fn a_far_end_with_no_setup_has_nothing_written_into_it() {
            let session = Session::with_config(quick_grace()).await;

            session.emit(vec![line(1, PROMPT)]).await;

            assert_eq!(session.written(), "");
        }

        #[tokio::test]
        async fn the_setup_opens_one_block_headed_by_the_command_and_closes_it() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session
                .emit(vec![
                    line(1, SETUP),
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;

            let started = session.started();
            let setup = *started.last().expect("the setup opened a block");
            assert_eq!(
                session.headings().last(),
                Some(&Some(SETUP.to_owned())),
                "the heading is the command verbatim, so a listener finds exactly what ran"
            );
            assert!(
                session
                    .events()
                    .contains(&SessionEvent::CommandFinished { command_id: setup }),
                "and it closes before the user's first command: {:?}",
                session.events()
            );
        }

        #[tokio::test]
        async fn a_banner_before_the_prompt_does_not_get_acters_own_line_read_aloud() {
            let session = set_up().await;

            session
                .emit(vec![line(1, "Last login: Fri Aug 29 10:14:02 2026")])
                .await;
            session.emit(vec![line(2, PROMPT), line(3, SETUP)]).await;
            session.advance_to(1_000).await;

            assert!(
                session.rendered().contains(SETUP),
                "every byte is still in the buffer, where the disclosure can be read back:                  {:?}",
                session.rendered()
            );
            assert!(
                !said_aloud(&session).contains("PROMPT_COMMAND"),
                "and none of it reaches the listener: {:?}",
                session.announcements()
            );
        }

        #[tokio::test]
        async fn the_far_end_is_heard_again_once_the_setup_block_closes() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session
                .emit(vec![
                    line(1, SETUP),
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(2, PROMPT),
                    marker(Osc133Marker::CommandStart),
                    line(2, "echo hi"),
                    marker(Osc133Marker::OutputStart),
                    line(3, "hi"),
                ])
                .await;
            session.advance_to(1_000).await;

            assert!(
                said_aloud(&session).contains("hi"),
                "{:?}",
                session.announcements()
            );
        }

        #[tokio::test]
        async fn a_setup_that_is_never_answered_does_not_silence_the_session_for_good() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.advance_to(300).await;
            session.emit(vec![line(2, "the user's own output")]).await;
            session.advance_to(1_300).await;

            assert!(
                session
                    .events()
                    .contains(&SessionEvent::IntegrationUnavailable),
                "the grace period is what expired: {:?}",
                session.events()
            );
            assert!(
                said_aloud(&session).contains("the user's own output"),
                "{:?}",
                session.announcements()
            );
        }

        fn said_aloud(session: &Session) -> String {
            session
                .announcements()
                .into_iter()
                .filter_map(|said| match said {
                    Announcement::ReadAloud { text } => Some(text),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn the_setup_is_never_read_back_to_the_listener() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session
                .emit(vec![
                    line(1, SETUP),
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;

            assert!(
                !session.rendered().contains("PROMPT_COMMAND"),
                "the echo of Acter's own line is not output: {:?}",
                session.rendered()
            );
            assert!(
                !session
                    .announcements()
                    .iter()
                    .any(|said| matches!(said, Announcement::Failed { .. })),
                "an assignment succeeds, and a success says nothing: {:?}",
                session.announcements()
            );
        }

        #[tokio::test]
        async fn a_block_the_marker_opens_for_acters_own_line_is_still_named() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session
                .emit(vec![
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;

            assert_eq!(
                session.headings().last(),
                Some(&Some(SETUP.to_owned())),
                "a block nobody can name from the frontend is named from here"
            );
        }

        #[tokio::test]
        async fn a_block_the_marker_opens_for_the_users_line_keeps_the_frontends_heading() {
            let session = Session::with_config(quick_grace()).await;

            session.submit("quiet").await;
            session
                .emit(vec![
                    marker(Osc133Marker::PromptStart),
                    line(1, "> "),
                    marker(Osc133Marker::CommandStart),
                    marker(Osc133Marker::OutputStart),
                    line(2, "output"),
                ])
                .await;

            assert_eq!(session.headings(), vec![None]);
        }

        #[tokio::test]
        async fn an_interrupt_after_the_setup_has_closed_has_nothing_to_stop() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session
                .emit(vec![
                    line(1, SETUP),
                    marker(Osc133Marker::OutputStart),
                    marker(Osc133Marker::CommandEnd(Some(ExitCode(0)))),
                ])
                .await;

            assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
        }
    }
}
