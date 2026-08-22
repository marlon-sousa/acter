//! Service: `SessionService` — one session owned end to end. It is what turns the
//! transport, the terminal engine, the boundary tracker and the session actor from four
//! components that could be wired together into one that is, and it is the only place
//! that knows they belong to each other.
//!
//! Service and not controller by the module role rule. [`SessionActor`] remains the
//! controller — the per-session loop, with the pacing behavior — and what lives here is
//! the wiring, the correlation between submitted commands and observed blocks, and the
//! lifetime of both. Deleting the actor would lose business behavior; deleting this
//! would lose connectivity.
//!
//! **It depends only on ports.** [`Transport`], [`TerminalEngine`], [`Clock`] and
//! [`EventSink`] are all this file names, so `acter-core` still names no adapter crate
//! and a scripted far end, a local ConPTY and an SSH channel are all the same shape from
//! here.
//!
//! # Two tasks, one owner each
//!
//! The **actor task** is [`SessionActor::run`], unchanged: it already selects over its
//! input channel and its two timers.
//!
//! The **pump task** owns the transport, the engine, the tracker and the correlation
//! queue, and selects over three things: the bytes the transport pushes, the requests
//! [`SessionApi`] submits, and the integration grace period.
//!
//! One owner for the transport is a requirement about *ordering*, not about contention.
//! `Transport` is `Send` and not `Sync` with `&mut self` on every method, so it has
//! exactly one owner by construction — and making the pump that owner is what keeps
//! writes ordered against reads: a device-query answer must never overtake a submitted
//! line. A `Mutex<dyn Transport>` shared between the router and a reader task was
//! rejected for exactly that reason (spec B6, decision 2).
//!
//! `SessionApi`'s methods stay synchronous — the trait is sync and dyn-compatible by
//! ARCHITECTURE's rule, and an invoke never waits on the shell — so they `try_send` onto
//! the request channel and return.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::select;
use tokio::spawn;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender, channel, unbounded_channel};

use crate::{
    BoundaryEvent, BoundaryTracker, Clock, CommandId, EventSink, ExitCode, Integration, KeyAck,
    KeyPress, LineId, LineRevision, PacingConfig, Region, Screen, SessionActor, SessionApi,
    SessionEvent, SessionId, SessionInput, SessionIntent, SubmitAck, TerminalEngine, Timer,
    Transport, intent_for,
};

/// Read buffering between the transport and the pump. Bounded, so a far end that floods
/// faster than the domain can absorb is slowed down rather than queued without limit:
/// those bytes are already in the world, and back-pressure is the honest answer to them.
const READS: usize = 1024;

/// How many invokes may be in flight toward the pump. Generous, because a person types
/// one line at a time and every entry here is one keystroke or one submitted line.
const REQUESTS: usize = 64;

/// What a terminal sends when the user presses Enter: a carriage return.
///
/// Not a line feed, and this is not cosmetic. A real shell on a pseudoconsole **echoes a
/// line feed and never runs the line** — it is still waiting for the Enter that never
/// came — so with `\n` here every command in a real session would appear to be accepted
/// and then silently do nothing. The scripted far end hid it by accepting either byte,
/// which is why it took a real shell to find (spec B4).
const ENTER: char = '\r';

/// One session: the [`SessionApi`] the routers hold, and the handle on the two tasks
/// behind it.
///
/// Started once and torn down with the process. Several concurrent sessions are a
/// `session_manager` above this, post-convergence; [`SessionId`] is carried on every
/// call as it already was, and ignored, because there is exactly one.
pub struct SessionService {
    /// The pump's inbox. Bounded, and never awaited: a `SessionApi` method may not wait
    /// on the shell.
    requests: Sender<Request>,
    /// The correlation counter, shared with the pump so an id minted at submission and
    /// an id minted for an unclaimed block can never collide. Starts at 1, so 0 never
    /// appears as a real command — as the A3 fake also arranged.
    next_id: Arc<AtomicU32>,
    /// Whether a submitted command is outstanding, so [`SessionApi::send_key`] can
    /// answer "there was nothing to act on" without waiting on the pump.
    running: Arc<AtomicBool>,
    /// Where events go, once the frontend has said where that is.
    sink: Arc<AttachedSink>,
}

impl SessionService {
    /// Starts the session: the far end begins talking, the actor begins listening, and
    /// the grace period starts running.
    ///
    /// Before any frontend has attached, deliberately. A shell draws its prompt when it
    /// starts rather than when a window is ready, and the grace period measures from
    /// session start because that is what it is about. Events produced before an attach
    /// reach nobody, which for a prompt is exactly right.
    ///
    /// Must be called from within a tokio runtime — the same requirement
    /// [`Transport::start`] and [`Clock::timer`] already carry, for the same reason.
    pub fn start(
        mut transport: Box<dyn Transport>,
        engine: Box<dyn TerminalEngine + Send>,
        clock: Arc<dyn Clock>,
        config: PacingConfig,
    ) -> Self {
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
        spawn(
            Pump {
                transport,
                engine,
                tracker: BoundaryTracker::new(),
                grace: config.integration_grace,
                clock,
                reads,
                inbox,
                inputs,
                next_id: Arc::clone(&next_id),
                running: Arc::clone(&running),
                integration: Integration::Pending,
                submitted: VecDeque::new(),
                echo: Echo::default(),
                open: None,
                interrupted: false,
                lines: HashMap::new(),
                last_line: None,
            }
            .run(),
        );

        Self {
            requests,
            next_id,
            running,
            sink,
        }
    }
}

impl SessionApi for SessionService {
    fn attach_session(&self, _session: SessionId, sink: Arc<dyn EventSink>) {
        self.sink.attach(sink);
    }

    /// Mints the correlation id and hands the line to the pump.
    ///
    /// The id is minted here rather than where the block opens, because the frontend
    /// needs it before anything has run: it opens the buffer block with it. Correlation
    /// is then the pump's queue of these, claimed at `BlockStarted` (spec B6,
    /// decision 3).
    ///
    /// Which block claims a queued id is settled by what the shell echoes for it
    /// (spec B6.1, decision 3), so the text travels with the id.
    ///
    /// A full or closed request channel still returns the ack: the id was minted, and an
    /// invoke's contract is to answer immediately. A closed one means the session has
    /// ended, which the frontend learns from the events it stops receiving.
    fn submit_command(&self, _session: SessionId, line: &str) -> SubmitAck {
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let accepted = self.requests.try_send(Request::Submit {
            command_id,
            line: line.to_owned(),
        });
        if accepted.is_ok() {
            // Outstanding from the moment the submission is accepted rather than from
            // the moment the pump reaches it: to the person who pressed Enter, the
            // command is running now.
            self.running.store(true, Ordering::SeqCst);
        }
        SubmitAck { command_id }
    }

    /// The keybinding table, then what the session is doing, then the pump.
    ///
    /// Both questions are answered without waiting on anything: the binding is a pure
    /// policy, and whether something is running is a fact the pump publishes as it goes.
    /// That reading can be a moment stale — but so can any answer, since the command may
    /// end between the keypress and the invoke, which is exactly why decision 7 has the
    /// service target whatever is running instead of an id the frontend supplied.
    fn send_key(&self, _session: SessionId, key: KeyPress) -> KeyAck {
        let Some(intent) = intent_for(&key) else {
            return KeyAck::Unbound;
        };
        match intent {
            SessionIntent::Interrupt => {
                if !self.running.load(Ordering::SeqCst) {
                    return KeyAck::NothingToActOn;
                }
                // A pump that is gone has nothing running by definition, so a failed
                // send has the same honest answer.
                match self.requests.try_send(Request::Interrupt) {
                    Ok(()) => KeyAck::Applied,
                    Err(_) => KeyAck::NothingToActOn,
                }
            }
        }
    }
}

/// Something the frontend asked for, on its way to the one task that may touch the
/// transport.
enum Request {
    Submit { command_id: CommandId, line: String },
    Interrupt,
}

/// The event sink the actor writes to, which forwards to whichever sink the frontend has
/// attached.
///
/// The session starts before any frontend exists and outlives a webview reload, while
/// `attach_session` may be called more than once — a reload re-establishes the Channel.
/// So the actor is handed something stable, and this holds the part that changes.
#[derive(Default)]
struct AttachedSink(Mutex<Option<Arc<dyn EventSink>>>);

impl AttachedSink {
    fn attach(&self, sink: Arc<dyn EventSink>) {
        *self.0.lock().expect("sink lock poisoned") = Some(sink);
    }
}

impl EventSink for AttachedSink {
    /// The lock is released before the forwarded send, so nothing a sink does while
    /// sending can deadlock against an attach.
    fn send(&self, event: SessionEvent) {
        let attached = self.0.lock().expect("sink lock poisoned").clone();
        if let Some(sink) = attached {
            sink.send(event);
        }
    }
}

/// The task that owns the far end: bytes in, domain facts out, and the only thing in the
/// system that may write to the transport.
struct Pump {
    transport: Box<dyn Transport>,
    engine: Box<dyn TerminalEngine + Send>,
    tracker: BoundaryTracker,
    grace: Duration,
    clock: Arc<dyn Clock>,
    reads: Receiver<Vec<u8>>,
    inbox: Receiver<Request>,
    /// Facts for the actor. Unbounded because dropping one is never acceptable — a lost
    /// `Output` is lost text, this product's cardinal defect — and because the actor is
    /// the single consumer and never waits on anything itself.
    inputs: UnboundedSender<SessionInput>,
    next_id: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    /// This session's integration status: the same two transitions the actor applies to
    /// the [`SessionState`](crate::SessionState) it owns. Kept here as well because the
    /// pump has to decide, at submission time, whether to open a command itself
    /// (decision 10). Both copies are driven by the same two facts and the pump is the
    /// source of both, so they cannot come to disagree.
    integration: Integration,
    /// Submissions minted and not yet claimed by a block, oldest first. Each carries the
    /// line it was minted for, because the shell's echo of a line is what identifies the
    /// submission it is running (spec B6.1, decision 3).
    submitted: VecDeque<Submitted>,
    /// What the shell has echoed since the last prompt: the B..C region, read as it
    /// arrives so it is complete by the time the block opens.
    echo: Echo,
    open: Option<CommandId>,
    /// Whether an interrupt was asked for and the command it was aimed at has not closed
    /// yet. What tells a stopped command from a finished one, since the exit code cannot
    /// (decision 8).
    interrupted: bool,
    /// What has been done with each line of the running command: `false` once some of
    /// its text has been forwarded, `true` if it was rewritten and its final text is
    /// still owed. Kept across regions on purpose — the engine settles a block's lines
    /// while the region is still `Output`, including the prompt row the echo was written
    /// onto, and it is knowing that row's id that keeps the echo out of the command's
    /// output.
    lines: HashMap<LineId, bool>,
    /// The last line whose text was forwarded, so consecutive lines are separated the way
    /// the pacing policy counts them.
    last_line: Option<LineId>,
}

impl Pump {
    /// Runs until the far end goes away or the session is torn down.
    async fn run(mut self) {
        let mut grace = Some(self.clock.timer(self.grace));
        loop {
            // Resolved before any state is touched, so no timer future is alive while
            // the step below mutates the pump.
            let woke = select! {
                read = self.reads.recv() => Woke::Read(read),
                request = self.inbox.recv() => Woke::Request(request),
                () = fire(&mut grace) => Woke::Grace,
            };
            match woke {
                // The far end let go: the shell exited, the connection dropped, the
                // scripted session ended. Not an error — the `Transport` port models the
                // end of a session as its channel closing.
                Woke::Read(None) => break,
                Woke::Read(Some(bytes)) => self.feed(&bytes).await,
                // Every `SessionApi` handle is gone, so nothing can ask for anything
                // again.
                Woke::Request(None) => break,
                Woke::Request(Some(request)) => self.request(request).await,
                Woke::Grace => {
                    grace = None;
                    self.grace_expired().await;
                }
            }
        }
    }

    /// One read, all the way through: bytes to items, items to boundary events, boundary
    /// events to the actor — and the engine's device-query answers back to the far end.
    async fn feed(&mut self, bytes: &[u8]) {
        let items = self.engine.advance(bytes);
        for event in self.tracker.observe(items) {
            match event {
                BoundaryEvent::MarkersObserved => {
                    self.integration = self.integration.markers_observed();
                    self.send(SessionInput::MarkersObserved);
                }
                BoundaryEvent::BlockStarted => self.block_started().await,
                BoundaryEvent::Line {
                    region,
                    id,
                    text,
                    revision,
                } => {
                    // The echo is read before anything else looks at the line, and it is
                    // read from every region: what the prompt put on a row is what tells
                    // a rewrite of that row apart from the command line on it.
                    self.echo.observe(region, &text, revision);
                    // `due` runs for every line whatever region it fell in: the
                    // bookkeeping it keeps is what tells the echo's row from the
                    // output's later on.
                    if let Some(text) = self.due(id, text, revision)
                        && self.wants(region)
                    {
                        self.output(id, text);
                    }
                }
                BoundaryEvent::BlockEnded { exit } => self.close(exit).await,
                BoundaryEvent::ScreenChanged(Screen::Alternate) => {
                    self.send(SessionInput::AltScreenEntered);
                }
                BoundaryEvent::ScreenChanged(Screen::Normal) => {
                    self.send(SessionInput::AltScreenLeft);
                }
            }
        }

        // An emulator does not answer a device query itself, and a program that asked one
        // waits forever if nobody writes the answer back — which for this product
        // surfaces as a session that has simply gone quiet.
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            self.write(&replies);
        }
    }

    async fn request(&mut self, request: Request) {
        match request {
            Request::Submit { command_id, line } => self.submit(command_id, &line).await,
            Request::Interrupt => self.interrupt(),
        }
    }

    /// A submitted line: correlated, then written.
    ///
    /// The id is queued for the block that will claim it — unless this session has no
    /// shell integration, in which case no block is coming and the submission is itself
    /// the boundary (decision 10). Either way the correlation is settled before the first
    /// byte goes out, so no output can arrive with nowhere to go.
    async fn submit(&mut self, command_id: CommandId, line: &str) {
        if self.integration == Integration::Unintegrated {
            self.close(None).await;
            self.open(command_id, None);
        } else {
            self.submitted.push_back(Submitted {
                id: command_id,
                line: line.to_owned(),
            });
        }
        self.write(format!("{line}{ENTER}").as_bytes());
        self.settle_running();
    }

    /// Asks the far end to stop what it is running, and remembers that it was asked.
    ///
    /// Remembering is the point: `BlockEnded { exit: None }` is either a bare `D` or a
    /// prompt reappearing mid-block, so the exit code cannot say whether a command was
    /// stopped. What the service just did can (decision 8).
    ///
    /// **It closes nothing, in any session.** B6 amended decision 10 so that an interrupt
    /// was itself the boundary in an unintegrated session, which made the stop timely to
    /// announce; B4.1 removed the announcement, and with it the amendment's only reason.
    /// What a close would cost is measured: the actor drops output that arrives while no
    /// command is active, so output arriving after a close is discarded — not rendered, not
    /// spoken — and what arrives after a working interrupt is the shell's own prompt
    /// coming back. That prompt is the whole answer the user gets, so closing here would
    /// leave silence, which is indistinguishable from a hung session. The block stays
    /// open, the prompt flows into it as ordinary output, and the next submission closes
    /// it — as stopped, because `interrupted` is still set.
    fn interrupt(&mut self) {
        self.interrupted = true;
        // A failed interrupt means the far end is already gone, which the closing read
        // channel is about to say properly.
        let _ = self.transport.interrupt();
    }

    /// A command's output region opened.
    ///
    /// Whatever the shell echoed for this block is taken here whether it is used or not:
    /// it belongs to the block that just opened, and leaving it behind would offer it to
    /// the next one.
    ///
    /// Two edges, answered rather than discovered. **The queue is empty**: the shell's
    /// own activity, or a forged `C`. A block genuinely opened and its output has to go
    /// somewhere, so a fresh id is minted and it is treated as a real command — dropping
    /// it would lose text. **A submission never opened a block**: see [`Pump::claim`],
    /// which is where B6.1 changed B6's answer.
    ///
    /// And one more, which only a recovering session reaches: a command is **already
    /// open** here. The tracker never nests blocks, so that can only be a command this
    /// pump opened itself at submission time while the session was unintegrated, and a
    /// late marker has just recovered it (DESIGN decision 8). The block is that same
    /// command finally announcing itself, so it keeps the id the ack already gave the
    /// frontend: the buffer block the user is looking at goes on to receive the real
    /// output and the real exit code, rather than being orphaned beside a second one.
    async fn block_started(&mut self) {
        let echoed = self.echo.take();
        if self.open.is_some() {
            self.last_line = None;
            return;
        }
        let command_id = self.claim(echoed.as_deref());
        self.open(command_id, echoed);
        self.settle_running();
    }

    /// Which submission this block is running.
    ///
    /// The front of the queue, unless the shell's own echo says otherwise. An echo that
    /// matches a *later* submission is the shell stating which line it read, and phase
    /// 1's shell is serial and reads lines in the order they were written — so the ones
    /// before it were already disposed of as something other than command lines: into a
    /// full-screen program, into a continuation, into a password prompt. They are not
    /// pending, they are over, and they are retired here (spec B6.1, decision 3).
    ///
    /// That is evidence, not the guess B6's decision 3 refused: retiring at the next
    /// prompt infers from a marker's *absence* that a line will never run, and a real
    /// shell draws its prompt before it reads the line a fast typist already sent.
    /// Matching is exact after trimming and never fuzzy, so a shell that echoes something
    /// else — or nothing — falls back to B6's claim from the front, and two identical
    /// submissions match at the front, which is right: the older one is running.
    ///
    /// With nothing queued at all, a fresh id: a block genuinely opened and its output
    /// has to go somewhere.
    fn claim(&mut self, echoed: Option<&str>) -> CommandId {
        if let Some(echoed) = echoed
            && let Some(index) = self
                .submitted
                .iter()
                .position(|submitted| submitted.line.trim() == echoed)
        {
            self.submitted.drain(..index);
        }
        self.submitted
            .pop_front()
            .map(|submitted| submitted.id)
            .unwrap_or_else(|| CommandId(self.next_id.fetch_add(1, Ordering::SeqCst)))
    }

    fn open(&mut self, command_id: CommandId, command_line: Option<String>) {
        self.open = Some(command_id);
        self.last_line = None;
        self.send(SessionInput::CommandStarted {
            command_id,
            command_line,
        });
    }

    /// Closes whatever command is open, as finished or as stopped.
    ///
    /// A block closing with no exit code while an interrupt is outstanding is a command
    /// the user stopped. Without one it still *ended* — stranding a session in "running"
    /// is the one answer that is certainly wrong (B2) — and reports exit code 0, which is
    /// also what a command that never ends structurally reports, having none.
    async fn close(&mut self, exit: Option<ExitCode>) {
        let Some(command_id) = self.open.take() else {
            return;
        };
        self.lines.clear();
        self.last_line = None;
        let stopped = exit.is_none() && self.interrupted;
        self.interrupted = false;
        self.send(if stopped {
            SessionInput::CommandInterrupted { command_id }
        } else {
            SessionInput::CommandEnded {
                command_id,
                exit_code: exit.unwrap_or(ExitCode(0)),
            }
        });
        self.settle_running();
    }

    /// The grace period elapsed. Whether that means anything is the actor's to decide —
    /// it owns the session state, and a session whose markers already arrived is
    /// unaffected.
    ///
    /// If a line was submitted during the grace period and no block ever claimed it, it
    /// becomes the degraded session's open command now, so everything arriving from here
    /// on has somewhere to go. Only the most recent one: the shell is serial, so the
    /// submissions before it are over.
    async fn grace_expired(&mut self) {
        self.integration = self.integration.grace_period_expired();
        self.send(SessionInput::GracePeriodExpired);
        if self.integration == Integration::Unintegrated
            && self.open.is_none()
            && let Some(latest) = self.submitted.pop_back()
        {
            self.submitted.clear();
            // No echo: a session with no markers has no B..C region for one to be in.
            self.open(latest.id, None);
        }
        self.settle_running();
    }

    /// Whether a line in this region is this command's output.
    ///
    /// With integration it is DESIGN's echo exclusion, which B2 promised would be the
    /// caller's one-line filter: block content is C..D and nothing else, so the prompt
    /// and the echo of the submitted line never reach the frontend as output. Without
    /// integration there are no regions at all, so the filter is the other one available
    /// — every line, echo included, because excluding the echo there would mean
    /// excluding everything (decision 10).
    fn wants(&self, region: Region) -> bool {
        match self.integration {
            Integration::Unintegrated => region == Region::Unstructured,
            Integration::Pending | Integration::Integrated => region == Region::Output,
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
    /// separator is the service's, not the engine's: a line item carries no line ending,
    /// and the pacing policy counts lines.
    fn output(&mut self, id: LineId, text: String) {
        let separated = match self.last_line {
            Some(last) if last == id => text,
            None => text,
            Some(_) => format!("\n{text}"),
        };
        self.last_line = Some(id);
        self.send(SessionInput::Output { text: separated });
    }

    /// Publishes whether anything is outstanding, for [`SessionApi::send_key`] to read.
    fn settle_running(&self) {
        self.running.store(
            self.open.is_some() || !self.submitted.is_empty(),
            Ordering::SeqCst,
        );
    }

    /// A write nobody is waiting on the result of.
    ///
    /// The submitted line and the engine's device-query answers both go out through here,
    /// and a failure means the far end is gone — which the read channel closing reports
    /// properly a moment later. Saying it out loud instead needs an event carrying a
    /// [`TransportError`](crate::TransportError), and the transport that can fail in
    /// interesting ways is B4's.
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.transport.write(bytes);
    }

    /// One domain fact for the actor. A closed channel means the actor is gone, which
    /// happens only when the session is being torn down.
    fn send(&self, input: SessionInput) {
        let _ = self.inputs.send(input);
    }
}

/// A submitted line waiting for the block that will run it.
///
/// The line travels with the id because the shell's echo of a line is the only thing
/// that identifies *which* submission a block is running (spec B6.1, decision 3).
struct Submitted {
    id: CommandId,
    line: String,
}

/// The command line the shell echoed, read as it arrives.
///
/// The B..C region is the shell saying which line it read, and it is the only honest
/// source for a block's heading: the text the frontend put there at submission time is an
/// optimistic guess that a drifted id can attach to the wrong block (spec B6.1,
/// decision 1).
///
/// Reading it is three rules. An **append** carries only the delta the extractor
/// computed, so the prompt the echo was written after is already excluded and the deltas
/// simply accumulate. Anything **else** carries the whole row, prompt included — the row
/// the prompt was drawn on is the row the echo is written onto — so the prompt this type
/// watched being drawn is stripped from the front of it, and if it does not match, the
/// command line becomes **unknown**: a heading that might contain the prompt is worse
/// than no correction at all.
///
/// Everything resets on a line outside the command-line region, which is what separates
/// one command's echo from the next: between two commands the tracker reports `Output`,
/// then `Unstructured`, then `Prompt`. Two command-line rows with no prompt between them
/// accumulate together, which is the right answer for a continuation line — the block's
/// command line genuinely is both rows.
#[derive(Default)]
struct Echo {
    /// What the prompt region put on the row the echo is being written onto.
    prompt: String,
    /// The command line so far, or `None` once something unreadable happened to it.
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
                // The whole row, so the prompt has to come off the front of it.
                LineRevision::Rewritten | LineRevision::Settled => {
                    self.text = text.strip_prefix(self.prompt.as_str()).map(str::to_owned);
                }
            },
            // A new prompt is a new command line, and what it draws is the prefix the
            // echo will be written after.
            Region::Prompt => {
                self.text = Some(String::new());
                match revision {
                    LineRevision::Appended => self.prompt.push_str(text),
                    LineRevision::Rewritten | LineRevision::Settled => {
                        self.prompt = text.to_owned();
                    }
                }
            }
            // Output, or text belonging to no block at all: whatever was echoed before it
            // belonged to a command that has already opened.
            Region::Output | Region::Unstructured => {
                self.text = Some(String::new());
                self.prompt.clear();
            }
        }
    }

    /// The command line for the block that just opened, and a clean slate for the next.
    ///
    /// Trimming is normalization and not interpretation: a row is padded with the spaces
    /// the grid holds. A line that trims away to nothing is a shell that echoed nothing,
    /// which is not a command line either.
    fn take(&mut self) -> Option<String> {
        let text = self.text.take().map(|text| text.trim().to_owned());
        self.text = Some(String::new());
        self.prompt.clear();
        text.filter(|text| !text.is_empty())
    }
}

/// What woke the pump. Resolved inside the `select!` so no borrow outlives it.
enum Woke {
    Read(Option<Vec<u8>>),
    Request(Option<Request>),
    Grace,
}

/// Awaits an armed timer, or waits forever when none is — so the grace branch simply
/// stops winning the select once it has fired, with no precondition to state.
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

    use crate::{Announcement, Key, Osc133Marker, TerminalItem, TransportError};

    use super::*;

    /// Time moves only when a test says so, and an armed timer fires only when its
    /// deadline is reached — B1.5's fake clock. Nothing here sleeps and nothing reads the
    /// real clock.
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

    /// Everything the frontend would have received.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<SessionEvent>>);

    impl EventSink for Recorder {
        fn send(&self, event: SessionEvent) {
            self.0.lock().expect("recorder poisoned").push(event);
        }
    }

    /// The far end, shared by the two fake driven ports so one handle drives both: the
    /// engine is told what the next read *meant*, and the transport records what the
    /// session said back to it.
    ///
    /// Items rather than bytes on the way in, because what a byte stream means is
    /// `acter-term`'s job and `pipeline.rs` already asserts that end to end over the real
    /// engine. What is under test here is everything above the engine.
    #[derive(Default)]
    struct FarEnd {
        reads: Mutex<Option<Sender<Vec<u8>>>>,
        batches: Mutex<VecDeque<Vec<TerminalItem>>>,
        written: Mutex<Vec<u8>>,
        interrupts: Mutex<u32>,
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

    /// One read, one batch: whatever the test queued for it, whatever the bytes were.
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
    }

    /// One session under test, with the handles that drive it.
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
            let far_end = Arc::new(FarEnd::default());
            let clock = Arc::new(FakeClock::default());
            let events = Arc::new(Recorder::default());
            let api = SessionService::start(
                Box::new(FakeTransport(Arc::clone(&far_end))),
                Box::new(FakeEngine(Arc::clone(&far_end))),
                Arc::clone(&clock) as Arc<dyn Clock>,
                config,
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

        /// Everything is in memory, so the tasks converge in a handful of turns; the
        /// bound turns a hang into a legible failure rather than a wedged suite.
        async fn settle(&self) {
            for _ in 0..64 {
                yield_now().await;
            }
        }

        /// One read arriving, meaning these items.
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
            ack.command_id
        }

        async fn press(&self, key: KeyPress) -> KeyAck {
            let ack = self.api.send_key(SessionId(1), key);
            self.settle().await;
            ack
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

        /// What the frontend would put on each block, in order.
        fn headings(&self) -> Vec<Option<String>> {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::CommandStarted { command_line, .. } => Some(command_line),
                    _ => None,
                })
                .collect()
        }

        fn rendered(&self) -> String {
            self.events()
                .iter()
                .filter_map(|event| match event {
                    SessionEvent::Output { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
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

    /// A whole command, marker for marker: the prompt, the echo of what was typed, the
    /// output, and the end.
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

    // --- Render before announce -----------------------------------------------------

    /// A5.2 pinned this inside the frontend controller, where one handler appended to
    /// the buffer and then spoke. A6 took the verdict off `Output`, so the ordering is
    /// now between two events and belongs to whoever emits them: the rendering event
    /// covering a span must precede the `Announce` about it, because the per-session
    /// channel delivers in order and the listener must never be read text the buffer
    /// does not have yet.
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

    // --- Correlation ----------------------------------------------------------------

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

    /// The shell's own activity, or a forged `C`. A block genuinely opened and its
    /// output has to go somewhere: dropping it loses text, which is the one thing this
    /// product must never do.
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

    /// What the far end is told when the user presses Enter, pinned as bytes.
    ///
    /// A carriage return, because that is what a terminal sends and what a real shell
    /// acts on: a bare line feed is echoed and then ignored, so a session would accept
    /// every command and run none of them. Found against a real `cmd.exe` in B4, and
    /// invisible until then because the scripted far end takes either byte as a line
    /// ending.
    #[tokio::test]
    async fn a_submitted_line_ends_with_what_a_terminal_sends_for_enter() {
        let session = Session::start().await;

        session.submit("git status").await;

        assert_eq!(session.written(), "git status\r");
    }

    /// The hole B6's decision 3 accepted, closed. It used to be that an id no block
    /// claimed stayed queued and the next block took it — and the queue never recovered,
    /// so from that point on every answer was heard under the question before it.
    ///
    /// The shell's own echo settles it: it says which line it is running, that line's id
    /// is claimed, and the line the shell has already disposed of is retired (spec B6.1,
    /// decision 3).
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

        // The queue recovered rather than staying one behind: the next command is its
        // own, not the retired one.
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

    /// The fallback, unchanged from B6: an echo that identifies no submission decides
    /// nothing and the block claims the front of the queue. Exact matching is the whole
    /// point — a shell that rewrote the line beyond recognition must not be able to
    /// retire an id on a resemblance.
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

    /// Retiring is also what unsticks the answer to a keystroke: `send_key` reads whether
    /// anything is outstanding, and a queue that can never drain reports "running"
    /// forever — so `Ctrl+C` could never again say there is nothing to stop.
    #[tokio::test]
    async fn a_session_that_retired_an_id_can_say_there_is_nothing_to_stop() {
        let session = Session::start().await;

        session.submit("typed into something else").await;
        session.submit("runs").await;
        session.emit(command(1, "runs", "output", Some(0))).await;
        session.advance_to(1_000).await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
    }

    // --- The block's heading --------------------------------------------------------

    /// Decision 1: the heading is what the shell echoed. The frontend's optimistic text
    /// makes the block appear the instant Enter is pressed; this is what has the last
    /// word on it.
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

    /// The echo arrives as the extractor's deltas, which a far end can spread across as
    /// many reads as it likes.
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

    /// A rewrite carries the whole row, prompt included — the prompt is drawn on the row
    /// the echo is written onto — so the prompt this session watched being drawn comes
    /// off the front of it (decision 2).
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

    /// And when the prompt does not explain the row, the command line is unknown rather
    /// than guessed: a heading that might carry the prompt inside it is worse than the
    /// one the frontend already has.
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

    /// A shell that emits `C` with nothing echoed before it: there is no command line to
    /// report, and an empty one would say the shell told us it was running nothing.
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

    /// One command's echo is never the next one's: the prompt between them starts the
    /// command line over, so a block that opens with nothing echoed says nothing rather
    /// than repeating the command before it.
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

    /// DESIGN's echo exclusion, which B2 promised would be a caller's one-line filter:
    /// the prompt the shell drew and the command line it echoed are both in the stream,
    /// and neither is this command's output.
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

    // --- Stopping -------------------------------------------------------------------

    /// The heart of decision 8: a block closing with no exit code while an interrupt is
    /// outstanding is a command the user stopped, and "finished, exit code 0" is the
    /// wrong announcement `CommandInterrupted` exists to avoid.
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
                .iter()
                .any(|event| matches!(event, SessionEvent::CommandFinished { .. })),
            "and never also as finished: {:?}",
            session.events()
        );
    }

    /// The other half of the same rule. A bare `D` is a bare `D`: with nothing
    /// outstanding it ends the command normally, because stranding a session in
    /// "running" is the one answer that is certainly wrong.
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

    /// The interrupt belongs to the command it was aimed at and does not follow the
    /// session around: the next command ends normally.
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

    // --- The keystroke surface ------------------------------------------------------

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

    /// A3.1 decision 6 named this: the typed `stop` had no honest way to say "nothing to
    /// stop", and an ack does.
    #[tokio::test]
    async fn a_bound_key_with_nothing_running_says_there_was_nothing_to_act_on() {
        let session = Session::start().await;

        assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
        assert_eq!(session.interrupts(), 0);
    }

    /// A command is outstanding from the moment Enter was pressed rather than from the
    /// moment its block opens: in between, there is certainly something to stop.
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

    // --- The grace period -----------------------------------------------------------

    /// Two hundred milliseconds instead of five seconds, so a test can wait for it
    /// without waiting for it. The number under test is the behavior, not the default,
    /// which `PacingConfig` pins on its own.
    fn quick_grace() -> PacingConfig {
        PacingConfig {
            integration_grace: Duration::from_millis(200),
            ..PacingConfig::default()
        }
    }

    #[tokio::test]
    async fn a_session_with_no_markers_is_flagged_when_the_grace_period_expires() {
        let session = Session::with_config(quick_grace()).await;

        session.advance_to(100).await;
        assert!(
            session.events().is_empty(),
            "nothing is said while the shell may still be starting: {:?}",
            session.events()
        );

        session.advance_to(300).await;
        assert_eq!(session.events(), vec![SessionEvent::IntegrationUnavailable]);
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

    /// DESIGN decision 8's recovery: a late marker upgrades the session, and from there
    /// blocks are trusted again.
    #[tokio::test]
    async fn a_late_marker_recovers_a_flagged_session() {
        let session = Session::with_config(quick_grace()).await;
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
            session.started(),
            vec![command_id],
            "the block that opened is the command that was submitted, not a second one"
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

    // --- Honest degradation ---------------------------------------------------------

    /// DESIGN's reliability case 2, Decided and until now happening nowhere: with no
    /// block there is no slot for the text, so the submission opens one and the next
    /// submission closes it.
    #[tokio::test]
    async fn an_unintegrated_session_makes_the_submission_the_boundary() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        let first = session.submit("one").await;
        session
            .emit(vec![line(1, "one"), line(2, "some output")])
            .await;
        session.advance_to(1_000).await;

        assert_eq!(session.started(), vec![first]);
        assert_eq!(
            session.rendered(),
            "one\nsome output",
            "every line reaches the buffer, the echo it can no longer exclude included"
        );
        assert!(
            session.announcements().is_empty(),
            "and nothing is read aloud: {:?}",
            session.announcements()
        );

        let second = session.submit("two").await;
        session.advance_to(2_000).await;

        assert_eq!(session.started(), vec![first, second]);
        assert!(
            session
                .events()
                .contains(&SessionEvent::CommandFinished { command_id: first }),
            "the previous command closed when the next line was submitted: {:?}",
            session.events()
        );
    }

    /// A line submitted while the shell might still have been starting: when the grace
    /// period resolves against it, that command opens rather than being stranded.
    #[tokio::test]
    async fn a_line_submitted_during_the_grace_period_opens_when_it_expires() {
        let session = Session::with_config(quick_grace()).await;

        let command_id = session.submit("early").await;
        session.advance_to(300).await;
        session.emit(vec![line(1, "output after the flag")]).await;
        session.advance_to(1_000).await;

        assert_eq!(session.started(), vec![command_id]);
        assert_eq!(session.rendered(), "output after the flag");
    }

    /// **B4.1, and the reason B6 decision 10's amendment came back out.** An interrupt in
    /// a session with no markers does not close the command: the block stays open, so the
    /// prompt coming back has somewhere to land and a second Ctrl+C still has something
    /// to aim at.
    ///
    /// The amendment made the interrupt the boundary so the stop could be announced while
    /// it was still news. Nothing announces it any more — the user hears the shell's own
    /// prompt — so the only thing a close still does here is throw that prompt away.
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

    /// The same rule with markers, where it was never in doubt: the `D` is the boundary,
    /// and the interrupt only records that one was asked for.
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

    /// **The regression closing would cause, pinned.** After a working interrupt what the
    /// far end sends is the shell's prompt coming back, and that prompt is the entire
    /// answer the user gets about whether the stop took effect — Acter says nothing of its
    /// own. The actor drops output arriving while no command is active, so a command
    /// closed by the interrupt would turn that answer into silence.
    #[tokio::test]
    async fn output_arriving_after_an_interrupt_still_reaches_the_frontend() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.submit("forever").await;
        session.emit(vec![line(1, "still working")]).await;
        session.press(ctrl('c')).await;

        // What a real `cmd.exe` sends after an interrupt that took effect: no `^C`, just
        // the prompt.
        session.emit(vec![line(2, r"C:\>")]).await;
        session.advance_to(1_000).await;

        assert!(
            session.rendered().contains(r"C:\>"),
            "the prompt that came back is what the user hears: {:?}",
            session.rendered()
        );
    }

    /// And the boundary that does come reports the truth about it. The next submission
    /// closes the interrupted command as stopped, not as finished with an invented exit
    /// code 0 — which is the mis-announcement `CommandInterrupted` exists to prevent.
    #[tokio::test]
    async fn the_next_submission_closes_an_interrupted_command_as_stopped() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        let command_id = session.submit("forever").await;
        session.emit(vec![line(1, "still working")]).await;
        session.press(ctrl('c')).await;

        session.submit("next").await;
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
                .iter()
                .any(|event| matches!(event, SessionEvent::CommandFinished { .. })),
            "and never also as finished: {:?}",
            session.events()
        );
    }

    // --- Attachment -----------------------------------------------------------------

    /// A webview reload re-establishes the Channel while the session keeps running.
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
}
