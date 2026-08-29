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
    BoundaryEvent, BoundaryTracker, Clock, CommandId, ConnectionState, EventSink, ExitCode,
    Integration, KeyAck, KeyPress, LineId, LineRevision, PacingConfig, Region, Screen,
    SessionActor, SessionApi, SessionEvent, SessionId, SessionInput, SessionIntent, ShellFacts,
    ShellMarkers, SubmitAck, TerminalEngine, Timer, Transport, intent_for,
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
    /// What this shell wants written when the user says there is no more input, taken
    /// from the adapter once at start.
    ///
    /// Held here rather than in the pump because the answer to the *invoke* depends on
    /// it: a shell with no measured answer must be told apart from one whose answer went
    /// out, and `KeyAck` is decided on this side of the channel (spec B5.2).
    eof: Option<Vec<u8>>,
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
    ///
    /// **The shell arrives as its adapter rather than as the one fact the session used to
    /// need.** B4.5 passed `ShellMarkers` here because the marker declaration was all the
    /// domain knew about a shell; B5.2 gave the same object a second domain-facing answer,
    /// and a composition root forwarding facts one at a time is the branch B5.1 deleted
    /// growing back a parameter at a time. Borrowed rather than held: both answers are
    /// read once, here, and nothing below this line asks a shell anything (spec B5.2).
    ///
    /// **What the shell says it needs run inside it goes out on the far end's first byte**,
    /// not here (spec B9.5, decisions 1 and 4). Bytes written before the shell has read them
    /// are an unmeasured race, and they are the launch-time injection wearing a different
    /// hat; what the pump already tracks is whether the far end has spoken, which is the same
    /// fact that makes a session "connected" at all.
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
        spawn(
            Pump {
                transport,
                engine,
                tracker: BoundaryTracker::new(markers),
                grace: config.integration_grace,
                clock,
                reads,
                inbox,
                inputs,
                next_id: Arc::clone(&next_id),
                running: Arc::clone(&running),
                integration: Integration::Pending,
                drawing: None,
                spoken: false,
                setup,
                discards_line,
                markers,
                submitted: VecDeque::new(),
                echo: Echo::default(),
                open: None,
                interrupted: false,
                lines: HashMap::new(),
                held: None,
                row: String::new(),
                last_line: None,
                cursor: None,
                pending_row: None,
            }
            .run(),
        );

        Self {
            requests,
            next_id,
            running,
            eof: shell.eof,
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
        // Always `Accepted`: a session that exists accepts. The other answer belongs to
        // the window that has no session at all, and is `ConnectService`'s to give
        // (spec B7, decision 3) — nothing here can be in that state.
        SubmitAck::Accepted { command_id }
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
            // Never gated on whether something is running, and that is the difference
            // from the interrupt above rather than an omission: the shell sitting at its
            // prompt is exactly who this is usually for, and a program reading standard
            // input is entitled to it too. What it is gated on is whether this shell's
            // answer was ever measured — a session over a shell Acter knows nothing about
            // says so rather than writing a byte and hoping (spec B5.2).
            SessionIntent::Eof => {
                let Some(bytes) = self.eof.clone() else {
                    return KeyAck::NothingToActOn;
                };
                match self.requests.try_send(Request::Eof { bytes }) {
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
    Submit {
        command_id: CommandId,
        line: String,
    },
    Interrupt,
    /// The shell's own end-of-input answer, carried rather than looked up: the pump owns
    /// the transport and nothing else, and which bytes end *this* shell is the service's
    /// to have asked the adapter once.
    Eof {
        bytes: Vec<u8>,
    },
}

/// The event sink the actor writes to, which forwards to whichever sink the frontend has
/// attached.
///
/// The session starts before any frontend exists and outlives a webview reload, while
/// `attach_session` may be called more than once — a reload re-establishes the Channel.
/// So the actor is handed something stable, and this holds the part that changes.
/// How many events are held for a frontend that has not attached yet.
///
/// Generous, because the window it covers is milliseconds to seconds — a shell starting
/// while a webview loads — and because the alternative is losing the opening of a session.
/// If it is ever reached, nothing is attached and nothing is going to be: a session with
/// ten thousand events and no frontend is headless, and holding more would only grow.
const BACKLOG: usize = 10_000;

#[derive(Default)]
struct Attached {
    sink: Option<Arc<dyn EventSink>>,
    /// What the session said before anyone was listening.
    backlog: Vec<SessionEvent>,
}

#[derive(Default)]
struct AttachedSink(Mutex<Attached>);

impl AttachedSink {
    /// Attaches, and hands over everything said before now.
    ///
    /// **The backlog is the whole point of this type since A9.** A session starts the
    /// moment the window does, and the frontend attaches when its page has loaded — so a
    /// shell that draws its prompt quickly does it into a sink nobody is holding. What was
    /// lost was not decoration: the session's first prompt, which is where a listener reads
    /// their working directory, and `ConnectionChanged`, which is how the window knows to
    /// stop saying "connecting". Both are emitted once and never repeated, so dropping them
    /// left the window permanently wrong rather than briefly late.
    ///
    /// A reload attaches again; by then the backlog is empty and this is the assignment it
    /// always was.
    fn attach(&self, sink: Arc<dyn EventSink>) {
        let mut attached = self.0.lock().expect("sink lock poisoned");
        attached.sink = Some(Arc::clone(&sink));
        // Sent while the lock is held, which is deliberate: see `send`.
        for event in std::mem::take(&mut attached.backlog) {
            sink.send(event);
        }
    }
}

impl EventSink for AttachedSink {
    /// **Forwarded under the lock, and that is a change of stance worth stating.** This
    /// used to clone the sink and release the lock first, so that nothing a sink did while
    /// sending could deadlock against an attach. Holding it costs that guarantee and buys
    /// ordering: with the lock released, an event sent during an attach could overtake the
    /// backlog being flushed beside it, and order is load-bearing here — render before
    /// announce, the verdict before the next prompt. Nothing this product attaches
    /// re-enters the sink while sending (the frontend's is a Tauri channel), so the
    /// deadlock the old comment guarded against is not reachable, while the reordering is.
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
    /// The prompt the far end is drawing, accumulated across the lines it arrives on, and
    /// emitted once it is complete (spec B5.6).
    ///
    /// **Kept here rather than read out of the echo tracker**, which keeps its own copy for
    /// a different job: that one exists to be stripped off the front of a rewritten row, so
    /// it is cleared and rebuilt on rules that suit *that* question and would be wrong for
    /// this one. Two readers of the same bytes with different lifetimes is the shape B4.5
    /// warned about, so each keeps what it needs.
    drawing: Option<String>,
    /// Whether the far end has said anything yet.
    ///
    /// **What makes a session "connected" is the far end speaking**, not a process having
    /// been spawned (spec A9, decision 3). A shell that was launched and has not drawn a
    /// prompt is not one anybody can use, and a window that called it connected would be
    /// telling a listener to go ahead and type.
    spoken: bool,
    /// The line to submit once the far end has spoken, taken once and never again.
    ///
    /// **The whole of B9.5 as far as this file is concerned.** Nothing is armed at launch any
    /// more, so what makes a far end mark its boundaries is this line arriving *after* the
    /// user's own startup files have had their say — which is the only ordering in which
    /// Acter has the last word rather than the first (spec B9.5, decision 1).
    ///
    /// `None` for a far end that is not being set up: one running a shell nobody has measured
    /// a setup for, one whose Connect dialog checkbox was unticked, and one whose dialog was
    /// cancelled. All three are the same thing from here — a session that runs and is told
    /// nothing.
    setup: Option<String>,
    /// What this shell's line editor reads as "discard whatever is pending on this line", or
    /// `None` for a shell that has no such byte (spec B4.5, decision 7).
    ///
    /// **The shell's own answer since B9.5, where it used to be inferred from the marker
    /// claim.** `PromptAndCommandLine` meant `cmd.exe` and said so exactly, until decision 8
    /// made POSIX `sh` the second shell to claim it — and an escape reaching a POSIX reader is
    /// a keypress rather than a discard. Measured 2026-08-29 against `docker-desktop`: the
    /// escape left busybox running a fragment of the line behind it.
    discards_line: Option<u8>,
    /// What the far end's prompt is able to say (spec B4.5). Only [`Pump::wants`] reads it
    /// here; the rest of the difference is the tracker's.
    markers: ShellMarkers,
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
    /// What has been done with each line seen so far: `false` once some of its text has
    /// been forwarded, `true` if it was rewritten and its final text is still owed. Kept
    /// across regions on purpose — the engine settles a block's lines while the region is
    /// still `Output`, including the prompt row the echo was written onto, and it is
    /// knowing that row's id that keeps the echo out of the command's output.
    ///
    /// Kept across *blocks* too, which is B4.2: without markers the engine is never told
    /// a boundary happened, so a row still on screen from a finished command keeps its id
    /// and settles with its full text partway through the next one. Forgetting it is what
    /// made that settlement look like a line nobody had ever seen.
    ///
    /// It cannot grow without bound: every id the engine emits as `Appended` is eventually
    /// emitted as `Settled` — by scrolling out of the screen area, by being swallowed as a
    /// continuation, at a block boundary, on a screen change or resize, or on staging
    /// saturation — and [`Pump::due`] removes the entry then. What this holds is the lines
    /// currently live on screen.
    lines: HashMap<LineId, bool>,
    /// Text that arrived somewhere it might not belong, and the line it came from.
    ///
    /// See [`Pump::hold`]: it is very often a submission's echo, and publishing it would
    /// mean the user's own command line read back at them — under a heading of its own at
    /// the start of a session, and as the previous block's output at every command after
    /// that.
    held: Option<(LineId, String)>,
    /// The tail of what the far end has appended lately, bounded by [`Pump::window`].
    ///
    /// Only [`Pump::boundary`] reads it: the far end's echo of a submitted line is
    /// recognised from accumulated text rather than from one append, so that a read
    /// cutting the echo in half cannot move the block boundary, and a command line wide
    /// enough to wrap is still recognised when its continuation lands on a new line item
    /// (spec B4.4).
    row: String,
    /// The last line whose text was forwarded, so consecutive lines are separated the way
    /// the pacing policy counts them.
    last_line: Option<LineId>,
    /// The line the far end last wrote to: where its cursor is, as far as anything here
    /// can know. Every item but a settlement moves it, because a settlement is the
    /// extractor freezing a row rather than the far end writing to one.
    cursor: Option<LineId>,
    /// The row a submission is pending on: [`Pump::cursor`] as it stood when the line was
    /// written to the far end (spec B4.9, decision 1).
    ///
    /// At that instant the far end has drawn its prompt and its cursor is on that row, and
    /// the only thing that reaches it is what this pump wrote — so everything appended to
    /// that row afterwards is the echo of it. That is exact rather than a match, which is
    /// why it can be dropped without any risk of hiding output, and it needs no markers,
    /// which is why it reaches inside a container, an `ssh` or a REPL.
    ///
    /// Only meaningful while something is pending; [`Pump::pending_echo`] asks both
    /// questions together.
    pending_row: Option<LineId>,
}

impl Pump {
    /// Runs until the far end goes away or the session is torn down.
    ///
    /// **The grace period is not armed here** (spec B9.5, decision 5). It used to run from
    /// `SessionService::start`, and a cold WSL distribution takes five to six seconds to say
    /// anything at all — so a session that was going to be set up perfectly well heard the
    /// unintegrated sentence first and recovered from it silently. `integration_grace` asks
    /// how long the far end has been talking without marking anything; a far end that has not
    /// spoken has not had its chance, and the clock should not be running.
    async fn run(mut self) {
        let mut grace = None;
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
                Woke::Read(None) => {
                    // The window stops saying it is connected to something that is gone,
                    // and keeps saying so rather than announcing it once and then looking
                    // like a working session (spec A9, decision 4).
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
                        // Armed here rather than at session start, because this is the moment
                        // the far end has had its chance (spec B9.5, decision 5).
                        grace = Some(self.clock.timer(self.grace));
                    }
                    self.feed(&bytes).await;
                    self.set_up().await;
                }
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
                } => self.line(region, id, text, revision).await,
                BoundaryEvent::BlockEnded { exit } => self.close(exit).await,
                BoundaryEvent::ScreenChanged(Screen::Alternate) => {
                    self.send(SessionInput::AltScreenEntered);
                }
                BoundaryEvent::ScreenChanged(Screen::Normal) => {
                    self.send(SessionInput::AltScreenLeft);
                }
            }
            // Checked inside the loop rather than after it: a batch can carry a whole
            // prompt and the command's output behind it, and the prompt has to be spoken
            // in the order it was drawn rather than after everything else in the read.
            self.prompt_finished();
        }

        // An emulator does not answer a device query itself, and a program that asked one
        // waits forever if nobody writes the answer back — which for this product
        // surfaces as a session that has simply gone quiet.
        let replies = self.engine.take_replies();
        if !replies.is_empty() {
            self.write(&replies);
        }
    }

    /// Sets the session up, once, on the far end's first byte.
    ///
    /// **It is submitted through the same path a typed line takes, and nothing about it is
    /// hidden** (spec B9.5, decision 3, decided by the user: *"this is just another command.
    /// Nothing will be hidden."*). It gets a block, the block's heading is the command
    /// verbatim, and the shell's own history keeps it — so a listener arrowing the buffer or
    /// pressing F6 finds exactly what ran.
    ///
    /// **Traced through, that is silent**: opening a block says nothing, a successful setup
    /// prints nothing, and since A6 decision 2 a successful command's exit code is not on the
    /// wire at all. What a listener hears is the connection sentence and then the prompt.
    ///
    /// The id is minted here rather than by [`SessionApi::submit_command`] for the reason
    /// [`Pump::unclaimed`] mints one: it shares the same counter, so an id minted for Acter's
    /// own line can never collide with one minted for the user's.
    ///
    /// **It waits for the far end to have drawn something, not merely to have spoken, and a
    /// real distribution is what taught that difference.** Measured 2026-08-29 against Ubuntu
    /// 24.04 under WSL: bash's first read carried bytes that produced no line at all, so a
    /// setup sent on it was pending before the pump knew which row the echo would be written
    /// onto — and the prompt that arrived next was held with the echo instead of being
    /// published in front of it. What a listener got was Acter's own five-hundred-character
    /// command read aloud between the connection sentence and the prompt.
    ///
    /// A cursor is exactly B4.9 decision 1's precondition: at that instant the far end has
    /// drawn its prompt, so everything appended to that row afterwards is the echo of what
    /// this pump writes next. Waiting for it costs nothing — a far end that draws nothing has
    /// no prompt to mark, and the grace period is already running.
    async fn set_up(&mut self) {
        if self.cursor.is_none() {
            return;
        }
        let Some(line) = self.setup.take() else {
            return;
        };
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.submit_line(command_id, &line, true).await;
    }

    async fn request(&mut self, request: Request) {
        match request {
            Request::Submit { command_id, line } => self.submit(command_id, &line).await,
            Request::Interrupt => self.interrupt(),
            Request::Eof { bytes } => self.end_input(&bytes),
        }
    }

    /// Writes the shell's end-of-input answer, and nothing else happens here.
    ///
    /// **No correlation id and no block**, deliberately. A submission is a command line
    /// the user composed and is owed a heading and a verdict; this is a keystroke, and
    /// giving it a block would put a command in the buffer that nobody typed. What the
    /// far end does with the bytes it then echoes and runs, so a session ending this way
    /// is still audible — measured against both PowerShell editions, where the answer is
    /// the line `exit` and the last thing the user hears is their session ending rather
    /// than silence (spec B5.2).
    ///
    /// **No cancel byte ahead of it either.** `cancel_pending_input` is for a line that
    /// would otherwise be concatenated onto input the user never typed, and its gate is a
    /// shell whose line editor discards on escape; the shell this arrived for is not one,
    /// and a shell that is has no measured end-of-input answer to send.
    fn end_input(&mut self, bytes: &[u8]) {
        self.write(bytes);
    }

    /// A submitted line: correlated, then written.
    ///
    /// The id is queued for the block that will claim it, in every session. **Pressing
    /// Enter no longer opens a block** — decision 10's second branch is gone (spec B4.4).
    /// A submission the far end never reads is not a command that ran, and opening a block
    /// for it produced the empty heading 22.10 found, with a later backlog filling
    /// whichever block happened to be open last.
    ///
    /// What opens the block instead is the far end echoing the line, which is the same
    /// evidence B6.1 already used to say *which* submission a block is running — the echo
    /// does for a session with no markers what `C` does for one with them. The correlation
    /// is still settled before the first byte goes out, so no output can arrive with
    /// nowhere to go.
    ///
    /// **An empty submission is written and nothing else** (spec B4.9, decision 4). It is
    /// a bare Enter, which is a re-orient gesture rather than a command: the shell redraws
    /// its prompt, the user hears where they are, and that is the whole of it. Queueing it
    /// would be worse than pointless — an empty line matches no echo, so the id would sit
    /// at the front of the queue until some later block claimed it, which is B6.1's drift
    /// restored by a keystroke. Not queueing it also keeps `running` honest, so Ctrl+C
    /// after a bare Enter still answers that there is nothing to stop.
    async fn submit(&mut self, command_id: CommandId, line: &str) {
        self.submit_line(command_id, line, false).await;
    }

    /// The same, saying whether the line is Acter's own — see [`Submitted::ours`].
    async fn submit_line(&mut self, command_id: CommandId, line: &str, ours: bool) {
        // Before the queue is touched, so "nothing else was already pending" is still
        // answerable — and so the escape is written ahead of the line it protects.
        self.cancel_pending_input();
        if !line.trim().is_empty() {
            self.submitted.push_back(Submitted {
                id: command_id,
                line: line.to_owned(),
                ours,
            });
            self.pending_row = self.cursor;
        }
        self.write(format!("{line}{ENTER}").as_bytes());
        self.settle_running();
    }

    /// Discards whatever is pending on the far end's line, ahead of the line about to be
    /// submitted.
    ///
    /// **What this is really about, and the roadmap entry had the cause wrong.** 22.11
    /// recorded that Acter answers a program's cursor-position query itself and that its
    /// own answer lands unread in front of the next submitted line. Measured against a
    /// real `cmd.exe`, the query from a program below **never reaches Acter at all**:
    /// ConPTY intercepts it, answers it into the console input queue itself, and the only
    /// thing on the wire is that answer already echoed back as caret-notation text. So the
    /// bytes are not this pump's, and no ledger of what it wrote can see them coming.
    ///
    /// What is left is prevention, and one byte does it: `cmd.exe`'s line editor treats
    /// escape as "discard the pending line", so the queued answer is thrown away and the
    /// submitted line is read as itself. Measured both ways — without it the shell answers
    /// `'s not recognized as an internal or external command,`, naming a command the user
    /// never typed; with it the line runs and the caret text is erased from the row.
    ///
    /// **Two gates, and both are load-bearing.**
    ///
    /// *The shell has a byte for it.* Escape clearing the line is `cmd.exe`'s line editor, not
    /// a universal; a POSIX shell's reader takes it as a meta prefix. That is
    /// `ShellAdapter`'s knowledge and it is asked there since B9.5 — until then the marker
    /// declaration stood in for it, on the reasoning that `PromptAndCommandLine` meant cmd and
    /// said so exactly. Decision 8 ended that by making `sh` the second shell to claim it, and
    /// the cost was measured the same afternoon: an escape written ahead of the setup line
    /// left busybox executing a fragment of it, which answered `-sh: r-sh: not found` in front
    /// of somebody who could not see what had happened.
    ///
    /// *The shell is reading a line, and this is the line.* An escape reaching a program
    /// that reads raw input is a keypress — in `vim` it leaves insert mode — so it may only
    /// go to a shell sitting at its prompt. That is exactly `Prompt` or `CommandLine`: the
    /// prompt has been drawn and nothing has yet said output began.
    ///
    /// **This is what makes 22.5 the precondition for 22.11 rather than a neighbour of
    /// it.** Without markers there are no regions at all and the question has no answer;
    /// with them it is read straight off the tracker. Inside a REPL, a nested shell or a
    /// container the region is `Output` — the proxying command never ended — so the gate
    /// stays shut and those far ends are untouched. A second line typed behind one the far
    /// end has not accounted for is not at a prompt either, whatever the region says.
    ///
    /// **It is written on its own, never joined to the line**, and that is not tidiness.
    /// ConPTY translates input bytes into key events, and an escape immediately followed
    /// by a letter is `Alt`+that letter rather than a bare escape: sent as one write the
    /// line was still rejected, and sent as its own write it runs. Measured.
    ///
    /// **A bare Enter is protected the same way** (spec B4.9, decision 5), which is what
    /// the third gate now says out loud: nothing else was already pending. It read
    /// `submitted.len() == 1` while every submission was queued, and an empty one no
    /// longer is. Without it the re-orient gesture returns garbage — the queued answer is
    /// submitted as a command line, and instead of the prompt the user hears that
    /// something they never typed is not recognized as an internal or external command.
    fn cancel_pending_input(&mut self) {
        let Some(cancel) = self.discards_line else {
            return;
        };
        let reading = matches!(self.tracker.region(), Region::Prompt | Region::CommandLine);
        if reading && self.submitted.is_empty() {
            self.write(&[cancel]);
        }
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
    /// And one more, which two different sessions reach in opposite directions: a command
    /// is **already open** here.
    ///
    /// **With nothing queued it keeps the id**, which is DESIGN decision 8's recovery. The
    /// open block is one this pump inferred for a submission whose echo it recognised, and
    /// a late marker has just recovered the session; the block is that same command
    /// finally announcing itself, so the buffer block the user is looking at goes on to
    /// receive the real output rather than being orphaned beside a second one.
    ///
    /// **With a submission still queued it closes and opens a fresh one**, which is where
    /// B4.5 arrives. In a marked `cmd.exe` session the first prompt of the session gets a
    /// block of its own — text that belongs to no command anyone submitted, exactly as
    /// DESIGN says — and the synthesized `C` that follows is a real boundary for a real
    /// submission. Keeping the prompt's block there would file the command's output under
    /// the session banner and leave the submission running forever.
    async fn block_started(&mut self) {
        let echoed = self.echo.take();
        if self.open.is_some() && self.submitted.is_empty() {
            self.last_line = None;
            return;
        }
        self.close(None).await;
        let claimed = self.claim(echoed.as_deref());
        // **Named by the shell's echo when there is one, and by Acter's own line when there
        // is not** (spec B9.5, decision 3). `claim` used to answer with an id and drop the
        // line it came from, so a block opened here rather than by [`Pump::boundary`] reached
        // the frontend with no heading at all. For a line the user typed that is the right
        // answer and B6.1's — the frontend's own heading stands. For the setup line, which no
        // frontend ever submitted, it is the empty level 2 heading B4.4's NVDA pass found.
        let named = echoed.or_else(|| {
            claimed
                .ours
                .then_some(claimed.line)
                .filter(|line| !line.trim().is_empty())
        });
        self.open(claimed.id, named);
        self.settle_running();
    }

    /// One line item, all the way to the frontend or deliberately not.
    ///
    /// Three questions, and their order is the whole of it (spec B4.4).
    ///
    /// **Is this append the far end echoing a line we submitted?** Then it is not output,
    /// it is the boundary: the block it belongs to opens here and takes the echo as its
    /// heading. Matching is B6.1's — exact after trimming, never fuzzy — and it is asked
    /// only where the tracker has not already delimited the echo itself. In an integrated
    /// session's `B..C` region [`Echo`] owns that job and [`Pump::wants`] already rejects
    /// the text; inside a nested shell there is no such region at all, because the
    /// proxying command never ends and everything the container writes lands in one open
    /// `C..D`.
    ///
    /// **Has this text anywhere to go?** A line arriving with no block open is dropped
    /// outright by the actor, which returns early when nothing is active — this product's
    /// cardinal defect. So a block is opened for whatever is waiting, or minted if nothing
    /// is, before a single character is forwarded. That is what makes deferring the open
    /// until the echo admissible: it can never cost text.
    ///
    /// **Does this region belong to the open block?** [`Pump::wants`], unchanged.
    /// Accumulates the prompt while the far end draws it, and emits it once it is done.
    ///
    /// **"Done" is the region changing away from `Prompt`**, which is `B` arriving — the
    /// shell saying it has finished drawing and is reading a command line. That is the
    /// moment a sighted user's prompt is on screen and complete, and it is before the user
    /// types anything, so a listener hears where they are while deciding what to run rather
    /// than after committing to it.
    ///
    /// Only a `Full` session emits: a shell marking less already has its prompt in the
    /// block as content (spec B4.5, decision 4), and saying it twice would be worse than
    /// the silence this fixes.
    fn drawn(&mut self, region: Region, text: &str, revision: LineRevision) {
        if self.markers != ShellMarkers::Full {
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

    /// Emits the prompt if the far end has finished drawing one.
    ///
    /// **Asked of the tracker's region rather than of the next line**, which is the thing
    /// this got wrong first: a prompt ends at `B`, and `B` is a marker rather than text, so
    /// waiting for another line meant the prompt was not announced until the *next* command
    /// produced output — one command late, and after the wrong verdict. The region is
    /// checked once per read instead, so the announcement lands in the same batch the shell
    /// finished its prompt in.
    ///
    /// A prompt of nothing but whitespace is not read out: some shells draw across two rows
    /// and the first of them is blank.
    fn prompt_finished(&mut self) {
        if self.tracker.region() == Region::Prompt {
            return;
        }
        if let Some(drawn) = self.drawing.take()
            && !drawn.trim().is_empty()
        {
            self.send(SessionInput::PromptDrawn { text: drawn });
        }
    }

    async fn line(&mut self, region: Region, id: LineId, text: String, revision: LineRevision) {
        self.drawn(region, &text, revision);

        // Read before anything else looks at the line, and read from every region: what
        // the prompt put on a row is what tells a rewrite of that row apart from the
        // command line on it.
        self.echo.observe(region, &text, revision);

        // Where the far end's cursor is, kept from every region for the same reason: the
        // row a submission will be echoed onto is a physical fact about the screen, and
        // whichever region the prompt happened to be labelled with does not change it.
        if revision != LineRevision::Settled {
            self.cursor = Some(id);
        }

        // `due` runs for every line whatever region it fell in: the bookkeeping it keeps
        // is what tells the echo's row from the output's later on.
        if let Some(due) = self.due(id, text.clone(), revision)
            && self.wants(region)
        {
            // Nothing is forwarded into a session with no open block: `SessionActor`
            // returns early when nothing is active, so that text would be dropped
            // outright — this product's cardinal defect. And nothing is forwarded onto
            // the row a submission is pending on, because what lands there is the user's
            // own line coming back (spec B4.9, decision 2).
            match self.open {
                Some(_) if !self.pending_echo(id) => self.output(id, due),
                _ => self.hold(id, due).await,
            }
        }

        self.boundary(region, id, text, revision).await;
        if self.window().is_none() {
            self.spill().await;
        }
    }

    /// Text that arrived where it might not belong: with no block open, or on the row a
    /// submission is pending on.
    ///
    /// **Held rather than published, while a submission is still waiting for its echo**,
    /// because that text very often *is* the echo. With no block open, publishing it put
    /// the user's own command line under a heading with no text — found in the NVDA pass
    /// for B4.4, where the listener's buffer ended with an empty level 2 heading and the
    /// command line repeated beneath it. With a block open it is worse, and it is what
    /// B4.9 is about: the line is read back at the user as the previous command's output,
    /// on every command of an unintegrated session and every line typed into a container.
    ///
    /// The hold is bounded twice over, because held text is text the listener has not
    /// heard yet: by [`Pump::window`], so it can never exceed the longest line anyone is
    /// waiting for, and by there being a submission pending at all. Anything past either
    /// bound can no longer be part of an echo and is spilled immediately.
    ///
    /// That bound is also the answer to B4.4's objection to suppressing the echo at all —
    /// that it would mean holding every row back until it was complete, delaying speech
    /// and stranding text when a far end goes quiet mid-row. Only the pending row is ever
    /// held, and only for as long as a line of that length could still be arriving.
    async fn hold(&mut self, id: LineId, text: String) {
        let Some(window) = self.window() else {
            self.spill().await;
            self.publish(id, text);
            return;
        };
        if !self.held.as_ref().is_some_and(|(held, _)| *held == id) {
            self.spill().await;
        }
        match self.held.as_mut() {
            Some((_, accumulated)) => accumulated.push_str(&text),
            None => self.held = Some((id, text)),
        }
        if self
            .held
            .as_ref()
            .is_some_and(|(_, held)| held.len() > window)
        {
            self.spill().await;
        }
    }

    /// Whether this row is the one a submission is pending on, and so whether what is
    /// being appended to it is the far end echoing that submission back.
    ///
    /// Both halves are the question: a row that was the pending row for a submission that
    /// has since opened its block is an ordinary row again, which is what
    /// [`Pump::window`] answers here.
    fn pending_echo(&self, id: LineId) -> bool {
        self.pending_row == Some(id) && self.window().is_some()
    }

    /// Gives held text the block it turned out to deserve, no echo having claimed it.
    async fn spill(&mut self) {
        let Some((id, text)) = self.held.take() else {
            return;
        };
        self.publish(id, text);
    }

    /// Forwards text that has finished waiting, into the block that is open or into one
    /// minted for text no submission accounts for.
    fn publish(&mut self, id: LineId, text: String) {
        if self.open.is_none() {
            self.unclaimed();
        }
        self.output(id, text);
    }

    /// Opens a block for text no submission accounts for — the shell's own prompt, or its
    /// banner. A fresh id rather than [`Pump::claim`], deliberately: claiming would take
    /// the submission whose echo this very row may be about to complete, and the boundary
    /// would then find nothing to open. B6 already treats a block nobody submitted as a
    /// real block with real output.
    fn unclaimed(&mut self) {
        let command_id = CommandId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.open(command_id, None);
        self.settle_running();
    }

    /// Whether the row this append landed on has now become the far end's echo of a line
    /// we submitted — and if it has, the block that line runs in opens here.
    ///
    /// **Decided on the row's accumulated text, never on one append**, and that is the
    /// whole reason this is separate from forwarding. A pseudoconsole hands over whatever
    /// it has, so `dir /s` can arrive as one append or as six; matching an append would
    /// make the boundary land wherever a read happened to cut, and
    /// `every_session_says_the_same_thing_when_every_byte_is_its_own_read` exists to
    /// forbid exactly that.
    ///
    /// **The echo is suppressed, and it is [`Pump::hold`] above that makes that possible**
    /// (spec B4.9). B4.4 forwarded it to the block that was open when the far end wrote
    /// it, on the grounds that removing it would mean holding every row back until it was
    /// complete — delaying speech and stranding text when a far end goes quiet mid-row.
    /// That is true of a rule that holds every row, and this is not one: what is held is
    /// the row the submission is pending on, which is where the echo is written and
    /// nowhere else. A listener heard the difference immediately — every command after the
    /// first read the user's own typing back at them before answering it.
    ///
    /// Asked only where the tracker has not already delimited the echo itself. In an
    /// integrated session's `B..C` region [`Echo`] owns that job and [`Pump::wants`]
    /// already rejects the text; inside a nested shell there is no such region at all,
    /// because the proxying command never ends and everything the container writes lands
    /// in one open `C..D`.
    async fn boundary(&mut self, region: Region, id: LineId, text: String, revision: LineRevision) {
        if region == Region::CommandLine {
            self.row.clear();
            return;
        }
        // Nothing is waiting for an echo, so nothing can be one.
        let Some(window) = self.window() else {
            self.row.clear();
            return;
        };

        // An append carries the delta the extractor computed, so appends accumulate. A
        // rewrite and a settlement carry the row's *whole* text, so they replace what has
        // accumulated rather than being thrown away — and only on the row a submission is
        // pending on, which is the one row an echo is being written to (spec B4.10).
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

        // Held text that is this echo *is* this echo, so it is dropped rather than
        // published: the command line belongs in the heading below, not read back at the
        // user as the previous block's output. Whatever came before the echo on that row —
        // a prompt, a banner — is still text the far end wrote, and still reaches a block.
        let whole = (revision != LineRevision::Appended).then_some(text.as_str());
        if let Some((id, held)) = self.held.take() {
            match before_echo(&held, whole, submitted.line.trim()) {
                Some(before) if before.is_empty() => {}
                Some(before) => self.publish(id, before),
                None => self.publish(id, held),
            }
        }

        self.close(None).await;
        self.open(submitted.id, Some(submitted.line));
        self.settle_running();
        self.row.clear();
    }

    /// How much of the recent stream could still be an echo: the longest line waiting for
    /// one, plus the character in front of it that has to prove the match starts a word.
    ///
    /// **The window is what makes crossing rows safe.** A command line wider than the
    /// screen wraps, and whether the wrap stays on one line item is the far end's
    /// business rather than ours — measured 2026-08-22, `cmd.exe` swallows its
    /// continuation into the same `LineId` and a container's `sh` starts a new one — so
    /// the text is accumulated across consecutive appends instead of per row. Bounding it
    /// by the longest pending submission is what stops that from becoming a search over
    /// the whole session: nothing older than the longest line anyone is waiting for can
    /// match, and a wrap inserts no characters, so a line split across rows joins back
    /// into exactly what was submitted.
    fn window(&self) -> Option<usize> {
        self.submitted
            .iter()
            .map(|submitted| submitted.line.trim().len())
            .max()
            .filter(|longest| *longest > 0)
            .map(|longest| longest + 1)
    }

    /// Which pending submission this row is the far end's echo of, if any.
    ///
    /// The row ends with the submitted line, because the echo is written after whatever
    /// the prompt drew and nothing follows it. Trailing whitespace is ignored the way
    /// B6.1's matcher ignores it, and the character before the match must not be
    /// alphanumeric — without that, submitting `ls` would match an output row ending in
    /// `dlls`. A blank submission matches nothing, or every empty row would be somebody's
    /// bare Enter.
    ///
    /// It is a suffix rather than B6.1's whole-string equality because the prompt shares
    /// the row: `C:\Users\marlo>dir /s` is one line, and the part of it that is evidence
    /// is the end. The failure direction is a block that should not have opened — extra
    /// structure — and never text that is hidden.
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

    /// Takes that submission, retiring the ones before it.
    ///
    /// The same rule as [`Pump::claim`] and for the same reason: phase 1's shell is
    /// serial, so a far end echoing a later line has already disposed of the earlier ones
    /// as something other than command lines (spec B6.1, decision 3).
    fn adopt(&mut self, index: usize) -> Submitted {
        self.submitted.drain(..index);
        self.submitted
            .pop_front()
            .expect("the index came from this queue")
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
    /// With nothing queued at all, a fresh id and an empty line: a block genuinely opened and
    /// its output has to go somewhere.
    ///
    /// **It answers with the whole submission rather than only its id** (spec B9.5,
    /// decision 3). Dropping the line meant a block opened on this path — a `C` arriving for a
    /// submission whose echo was never recognised — reached the frontend as
    /// `command_line: None`, which is the empty level 2 heading B4.4's NVDA pass found: the
    /// listener's buffer ended with a heading that said nothing, and the command line was
    /// repeated as text beneath it.
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
        })
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
        // Every line still on record had its text forwarded to the block that is closing:
        // a rewrite only ever reaches `due` for a line that appended first. So nothing
        // here is owed to anyone any more, and saying so is what stops a row of this
        // command settling into the *next* one when no marker ever freezes it (B4.2).
        // Clearing instead would leave those settlements looking like lines never seen.
        self.lines.values_mut().for_each(|owed| *owed = false);
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
    /// **It no longer adopts a submission** (spec B4.4). It used to open a block for the
    /// most recent line submitted during the grace period, so that everything arriving
    /// afterwards had somewhere to go. [`Pump::line`] now guarantees that for every path
    /// rather than for this one — nothing is forwarded until a block exists — and adopting
    /// here would open exactly the empty heading 22.10 was about, for a line the far end
    /// may never have read.
    async fn grace_expired(&mut self) {
        self.integration = self.integration.grace_period_expired();
        self.send(SessionInput::GracePeriodExpired);
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
    ///
    /// **A session that has not been told yet is the third case, and leaving it out is
    /// the whole of 27.4** (spec B6.2). Before the first marker there is no structure to
    /// filter by: the tracker labels every line `Unstructured`, which the marked arm
    /// below wants nothing to do with — so a far end that had already drawn its prompt
    /// had it discarded. Not held, not rendered, gone, with no block and no buffer entry
    /// to find it in afterwards. Every unintegrated session in this product starts here
    /// and stays here for the whole grace period, which is why the symptom was a session
    /// that connected and then said nothing at all.
    fn wants(&self, region: Region) -> bool {
        match self.integration {
            Integration::Unintegrated => region == Region::Unstructured,
            // Unstructured text is wanted, **and** so is anything the marked filter
            // wants. The second half cannot fire today, because the first marker of a
            // session resolves `Pending` in the same batch it is seen in, so a region
            // that is not `Unstructured` is one reached while `Integrated`. It is spelled
            // out anyway: the alternative is a filter whose correctness rests on the
            // order of two arms in `Pump::feed`, and this one rests on nothing.
            Integration::Pending => region == Region::Unstructured || self.marked(region),
            Integration::Integrated => self.marked(region),
        }
    }

    /// What a session whose far end marks its boundaries wants, by what those markers are
    /// able to say.
    fn marked(&self, region: Region) -> bool {
        match self.markers {
            ShellMarkers::Full => region == Region::Output,
            // **The prompt is content in a shell that emits no `D`** (spec B4.5,
            // decision 4). There is no exit code to announce, so nothing at all is
            // said when a command ends, and the prompt coming back is the only ending
            // such a session has to offer a listener — which is why 22.12 records it
            // as a requirement rather than a nicety. An unintegrated session already
            // speaks it, as output of the block that is still open; marking the
            // session without this arm would take it away.
            ShellMarkers::PromptAndCommandLine => {
                matches!(region, Region::Output | Region::Prompt)
            }
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
            // The default is only honest because the record outlives the block: a line
            // whose text went to an earlier block is on record as owing nothing, rather
            // than being indistinguishable from one nobody has seen (B4.2).
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

/// What of the text held on the echo's row was *not* the echo that just matched.
///
/// `None` means none of it could be told apart, and the caller publishes the held text
/// whole: losing text is this product's cardinal defect, and a line the listener hears
/// twice is not.
///
/// **With the whole row in hand this is arithmetic** (spec B4.10). Held text is a tail of
/// the row the echo was written onto, and the echo is the end of that row, so where the
/// held text sits inside the row says how much of it lies in front of the echo. That much
/// is the prompt or the banner the far end drew before reading the line, and it is
/// published; the rest is the user's own line coming back.
///
/// **Without it, the strip B4.9 shipped**, which is all an echo matched on appends alone
/// allows: the held text either ends with the submitted line or is not that echo at all.
/// It is exactly the case the whole row is needed for — an echo whose last characters
/// arrived as a settlement rather than as an append — that the strip cannot see, because
/// the held text is then a line one character short of the one that matched.
fn before_echo(held: &str, row: Option<&str>, line: &str) -> Option<String> {
    let held = held.trim_end();
    let Some(row) = row.map(str::trim_end) else {
        return held.strip_suffix(line).map(str::to_owned);
    };
    let echo_at = row.len().checked_sub(line.len())?;
    // The held text is a tail of this row unless something rewrote the row underneath it,
    // in which case there is no arithmetic to do and the caller keeps every character.
    let held_at = row.rfind(held)?;
    let keep = echo_at.saturating_sub(held_at).min(held.len());
    Some(held[..keep].to_owned())
}

/// A submitted line waiting for the block that will run it.
///
/// The line travels with the id because the shell's echo of a line is the only thing
/// that identifies *which* submission a block is running (spec B6.1, decision 3).
struct Submitted {
    id: CommandId,
    line: String,
    /// Whether this line is Acter's own rather than the user's — and therefore whether the
    /// block it opens has to be named from here.
    ///
    /// **A line the user typed is already headed by the frontend**, which puts the text on the
    /// block the moment the submit ack answers; `command_line: None` then means "the shell did
    /// not say what it is running" and the heading the ack gave stands. Naming it from here
    /// with anything but the shell's own echo would overwrite it with a guess, which is the
    /// whole of B6.1's decision 1 — a drifted id must not be able to put the wrong words on a
    /// block.
    ///
    /// **Acter's own setup line has no ack and no frontend heading** (spec B9.5, decision 3).
    /// It is submitted by the pump on the far end's first byte, so a block opened for it by
    /// [`Pump::block_started`] rather than by [`Pump::boundary`] would reach the buffer with
    /// nothing on it at all — the empty level 2 heading B4.4's NVDA pass found, with the
    /// command line repeated as text beneath it.
    ours: bool,
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

    use crate::{Announcement, Key, Osc133Marker, SessionSetup, TerminalItem, TransportError};

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

    /// What a shell tells the service about itself, built by hand — which is all a fake
    /// needs to be, because this is a value rather than a port: knowledge, with nothing to
    /// record and nothing to script (spec B5.1 decision 2, and the value B5.2 bundled it
    /// into).
    fn marking(markers: ShellMarkers) -> ShellFacts {
        ShellFacts {
            markers,
            eof: None,
            setup: None,
            discards_line: None,
        }
    }

    /// The same, for the one shell whose line editor discards on a byte — which is `cmd.exe`,
    /// and which the session is now *told* rather than inferring from the marker claim.
    fn discarding_on(byte: u8) -> ShellFacts {
        ShellFacts {
            discards_line: Some(byte),
            ..marking(ShellMarkers::PromptAndCommandLine)
        }
    }

    /// The same, for a far end Acter is going to set up once it speaks — which since B9.5 is
    /// the only reason a WSL or SSH far end marks anything at all.
    fn set_up_with(line: &str) -> ShellFacts {
        ShellFacts {
            setup: Some(SessionSetup {
                line: line.to_owned(),
                markers: ShellMarkers::Full,
            }),
            ..marking(ShellMarkers::Full)
        }
    }

    /// A shell that answers end-of-input with these bytes. Deliberately not the answer
    /// PowerShell was measured to want: what the service must do is write *whatever the
    /// shell said*, and a fake spelling out the one real answer would let a service that
    /// hardcoded it pass.
    fn ending_with(bytes: &[u8]) -> ShellFacts {
        ShellFacts {
            markers: ShellMarkers::Full,
            eof: Some(bytes.to_vec()),
            setup: None,
            discards_line: None,
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
            Self::of(config, ShellMarkers::Full).await
        }

        /// A session over a far end whose prompt marks only what this says (spec B4.5).
        async fn of(config: PacingConfig, markers: ShellMarkers) -> Self {
            Self::over(config, marking(markers)).await
        }

        /// A session over a far end that is a particular shell, which since B5.2 is what
        /// the service is told about rather than one fact taken out of it. Most tests here
        /// care only about the markers and reach this through [`Self::of`].
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

        /// Everything one block received, concatenated. The boundary now falls on the
        /// far end's echo rather than on the submission, so which block a row landed in
        /// is the question most of these tests are actually asking.
        fn output_of(&self, command_id: CommandId) -> String {
            self.events()
                .into_iter()
                .filter_map(|event| match event {
                    SessionEvent::Output {
                        command_id: at,
                        text,
                        ..
                    } if at == command_id => Some(text),
                    _ => None,
                })
                .collect()
        }

        /// Each `Output` event's text in order, so what one block received can be told
        /// apart from the stream as a whole — which [`Session::rendered`] concatenates.
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

    /// What a shell has drawn when it is waiting for a line. Any string would do — what
    /// makes it a prompt in these tests is the region it falls in and the fact that the
    /// echo is appended to the same row.
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

    /// A row leaving the screen area, carrying its whole final text — what the engine
    /// emits for any live line that scrolls out, whichever command put it there.
    fn settled(id: u64, text: &str) -> TerminalItem {
        TerminalItem::Line {
            id: LineId(id),
            text: text.to_owned(),
            revision: LineRevision::Settled,
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
                .contains(&SessionEvent::CommandFinished { command_id }),
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

    /// **The clock starts when the far end speaks, not when the session starts** (spec
    /// B9.5, decision 5). `integration_grace` asks how long the far end has been talking
    /// without marking anything, and a far end that has not spoken has not had its chance.
    #[tokio::test]
    async fn a_session_with_no_markers_is_flagged_when_the_grace_period_expires() {
        let session = Session::with_config(quick_grace()).await;

        // The far end speaks, which is what starts the clock. Bytes that produce no line
        // item at all are the plainest way to say it — a shell clearing its screen, say.
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

    /// **A far end that has said nothing is never flagged**, however long anybody waits.
    ///
    /// Forced by decision 4 and correct on its own terms: the grace period used to run from
    /// `SessionService::start`, and a cold WSL distribution takes five to six seconds to say
    /// anything at all — so a session that was going to be set up perfectly well heard the
    /// unintegrated sentence first and recovered from it silently, which is roadmap 23.10's
    /// defect made systematic rather than incidental.
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

    /// **27.4, and the reason it was worth its own entry** (spec B6.2). What a far end
    /// says before it has marked anything is unstructured, not absent, and until this
    /// test it was thrown away: an SSH session drew its prompt, the frontend received two
    /// events and no output at all, and there was nothing in the buffer to go back and
    /// read. The prompt reaches the listener as the content of a block nobody submitted,
    /// which is what `Pump::unclaimed` is already for.
    #[tokio::test]
    async fn what_the_far_end_said_before_its_first_marker_reaches_the_frontend() {
        let session = Session::with_config(quick_grace()).await;

        session.emit(vec![line(1, "acter@acter-ssh:~$ ")]).await;
        // One rendering tick, and well inside the grace period: the listener meets the
        // prompt while the session is still deciding what it is, which is the point.
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

    /// The same text, and then the markers arrive after all — a shell that printed a
    /// banner before its integration ran. The banner is still the listener's, and the
    /// session is still integrated: forwarding what arrived before the first marker
    /// cannot be allowed to cost the filtering that starts at it.
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

    /// DESIGN decision 8's recovery: a late marker upgrades the session, and from there
    /// blocks are trusted again.
    #[tokio::test]
    async fn a_late_marker_recovers_a_flagged_session() {
        let session = Session::with_config(quick_grace()).await;
        // The far end speaks first, because that is what starts the clock this test is
        // waiting out (spec B9.5, decision 5).
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

    // --- Honest degradation ---------------------------------------------------------

    /// DESIGN's reliability case 2, Decided and until now happening nowhere: with no
    /// block there is no slot for the text, so the submission opens one and the next
    /// submission closes it.
    #[tokio::test]
    async fn an_unintegrated_session_makes_the_echo_the_boundary() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        // The shape a real shell produces: a prompt is drawn, and the echo of what the
        // user submits is written onto that same row.
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

    /// The other half of the same rule, and 22.10's defect: pressing Enter is not evidence
    /// of anything. A far end that never reads the line — a `docker run -t` holding a tty
    /// it never attaches stdin to — leaves nothing to echo, and a heading with nothing
    /// under it tells the user a command ran when none did.
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

    /// What B4.9 is about: the far end's echo of a submitted line is held on the row it
    /// is written onto and dropped, rather than forwarded as the previous block's output
    /// and read out at the user before their answer arrives.
    ///
    /// The rule is positional, not a match against the heading. Everything appended to the
    /// row the far end's cursor was on when Enter was pressed is the echo, because the
    /// only thing that reaches the far end is what this pump wrote — so it can be dropped
    /// with no risk of hiding output, which comparing text against the heading could never
    /// promise: running `dir` twice makes `dir` both a heading and a plausible output row.
    mod the_echo_is_not_read_back {
        use super::*;

        /// B4.4 fixed the *first* command of a session, where the echo is held for want of
        /// a block and dropped. Every command after it still had a block open, so the echo
        /// went straight into it — which is what a listener meets, one command in.
        #[tokio::test]
        async fn no_command_in_an_unintegrated_session_reads_the_typed_line_back() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.emit(vec![line(1, PROMPT)]).await;
            let first = session.submit("acter-one").await;
            session
                .emit(vec![line(1, "acter-one"), line(2, "first answer")])
                .await;

            // The next prompt, which is forwarded into the block that is still open — it
            // is the only ending an unintegrated session has to offer — and then the same
            // shape again, this time with a block open the whole way through.
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

        /// The seam B4.5 opened, and the reason this rule needs no markers. Inside a
        /// container the proxying command's `C..D` never closes, so everything the far end
        /// writes lands in that one open block — the echo included, before the boundary
        /// has recognised it.
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
                    // The container's own prompt, which no marker delimits.
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

        /// The bound that makes this safe, and B4.4's objection answered in a test: only
        /// the pending row is ever held, so output produced while a submission is pending
        /// is spoken the moment it arrives rather than waiting behind anything.
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

        /// The other half of the same bound. Text appended to the pending row that turns
        /// out not to be an echo is still the far end's, and is still spoken — held only
        /// while a line of that length could still be arriving.
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

    /// B4.10, and roadmap 22.13's measurement: an echo whose last characters never
    /// arrived as an append.
    ///
    /// A pseudoconsole cuts a read wherever it likes, and the read that carries the last
    /// character of an echo routinely carries the output after it too. Enough output and
    /// the echo's row leaves the screen area inside that one `advance`, so the extractor
    /// emits it as a **settlement carrying the whole row** rather than as the append that
    /// would have completed it. Measured in a real `alpine` container 2026-08-23: the row
    /// arrived complete, as `Settled`, and the matcher threw it away.
    ///
    /// The same session therefore said two different things depending on where the pipe
    /// cut — under byte-at-a-time reads every character appends and the block opens —
    /// which is what `every_session_says_the_same_thing_when_every_byte_is_its_own_read`
    /// forbids.
    mod an_echo_completed_by_a_whole_row_revision {
        use super::*;

        /// The measured case, in the shape the engine emitted it: the echo one character
        /// short as an append, then the whole row as a settlement, then the output that
        /// scrolled it away.
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

        /// The other revision that carries a whole row. A far end that repaints the row it
        /// is echoing onto — a line editor redrawing after a bracketed paste — says the
        /// same thing by rewriting rather than by settling.
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

        /// The gate, and why it is not decoration. Settlements arrive for **old** rows,
        /// out of order and long after they were written — a row settles when the screen
        /// scrolls past it. Run the same command twice and the first one's echo row is
        /// still on screen, ending in the line the second submission is waiting for; a
        /// rule that took any row's whole text would open the second block on a row the
        /// far end wrote minutes ago, before it had echoed anything at all.
        ///
        /// Position is what makes a whole row admissible, and only one row has it: the one
        /// the far end's cursor was on when Enter was pressed (spec B4.9).
        #[tokio::test]
        async fn a_settlement_on_any_other_row_opens_nothing() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            // The first `dir`, echoed and answered in the ordinary way.
            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("dir").await;
            session
                .emit(vec![line(1, "dir"), line(2, "one.txt"), line(3, PROMPT)])
                .await;
            session.advance_to(1_000).await;
            let opened = session.started().len();

            // The second, pending on row 3 — and row 1, which is the first command's echo,
            // scrolls out of the screen area while it waits.
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

        /// The half of B4.9 this could have undone. The held text is the echo one
        /// character short, so the strip that looks for the whole submitted line on the
        /// end of it finds nothing — and publishing it anyway is the user's own line read
        /// back at them, which is the defect B4.9 exists to have removed.
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

        /// And the other direction, which is the one that must never fail: text the far
        /// end wrote in front of the echo is still text, and still reaches a block.
        #[tokio::test]
        async fn what_the_far_end_wrote_in_front_of_the_echo_is_kept() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            // Nothing has been published for this row yet: the banner is drawn onto it
            // after the line was submitted, so it is held with the echo that follows it.
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

    /// A bare Enter: a re-orient gesture rather than a command (spec B4.9).
    ///
    /// The frontend used to drop it, so nothing was written, the shell never redrew its
    /// prompt and the user heard nothing at all. It is also ordinary input to a running
    /// program — a REPL, a "press Enter to continue" — which is the other half of why the
    /// guard was wrong.
    mod a_bare_enter {
        use super::*;

        /// Written, and queued for nothing. An empty line matches no echo, so an id
        /// queued for it could only be claimed by some later block — B6.1's drift,
        /// restored by a keystroke.
        #[tokio::test]
        async fn is_written_and_opens_no_block() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, PROMPT)]).await;

            session.submit("").await;
            // What a shell does with it: draws its prompt again, on a new row.
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

        /// Nothing is queued for it either, which is what keeps `running` honest: a
        /// submission that can never be claimed would otherwise leave the session
        /// answering that there is something to stop for the rest of its life.
        #[tokio::test]
        async fn leaves_nothing_running() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;

            session.submit("").await;

            assert_eq!(session.press(ctrl('c')).await, KeyAck::NothingToActOn);
            assert_eq!(session.interrupts(), 0, "and nothing was interrupted");
        }

        /// The drift, asserted directly: the next real command's output belongs to the
        /// next real command.
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

    /// **B4.2, written from the capture that found it.** A session that had run
    /// `dir /s C:\Windows\System32` and then `ping -n 20`: the finished `dir`'s rows are
    /// still live in the engine, because without markers nothing ever freezes them, and
    /// they scroll off the emulated screen one at a time as `ping`'s replies push them
    /// up. Each arrived settled, carrying its whole text, into the block the user was
    /// reading.
    ///
    /// One old row per new line of output is the signature, and it is what makes the
    /// buffer unreadable: nothing in the block says which lines the command produced.
    #[tokio::test]
    async fn a_finished_commands_rows_do_not_settle_into_the_next_block() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        // The prompt, then the echo written onto it, which is what opens a block now that
        // pressing Enter does not (spec B4.4).
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

    /// The case that decides between demoting the record at a boundary and merely keeping
    /// it. A row rewritten before the boundary is on record as still owing its final
    /// text; kept as-is, that text would be paid to whichever block happened to be open
    /// when the row scrolled out.
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

    /// And the case the "never seen means still owed" default exists for, which the fix
    /// must not take with it: a line that scrolled out of the screen area inside a single
    /// read arrives settled and complete, having never appended, and is this block's own
    /// output.
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
            vec!["still on screen\nscrolled past inside one read".to_owned()],
            "dropping a settlement nobody has a record of would lose output: {:?}",
            session.outputs()
        );
    }

    /// An integrated session is unaffected, and the ordering is why: the engine settles a
    /// block's lines *before* the `D` that closes it, so their records are spent by the
    /// time the boundary is applied and there is nothing left to demote.
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

    /// A line submitted while the shell might still have been starting: when the grace
    /// period resolves against it, that command opens rather than being stranded.
    #[tokio::test]
    async fn a_line_submitted_during_the_grace_period_opens_when_it_is_echoed() {
        let session = Session::with_config(quick_grace()).await;

        let command_id = session.submit("early").await;
        session.advance_to(300).await;
        // **B4.4 moved what resolves this.** The grace period expiring used to adopt the
        // most recent submission so that later output had somewhere to go; the far end
        // echoing the line does it now, and nothing is forwarded before a block exists
        // either way. The submission is no longer stranded — it is simply waiting for the
        // evidence every other submission waits for.
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
    async fn the_next_commands_echo_closes_an_interrupted_command_as_stopped() {
        let session = Session::with_config(quick_grace()).await;
        session.advance_to(300).await;

        session.emit(vec![line(1, "C:\\>")]).await;
        let command_id = session.submit("forever").await;
        session
            .emit(vec![line(1, "forever"), line(2, "still working")])
            .await;
        session.press(ctrl('c')).await;

        // The submission is no longer the boundary; the far end echoing the next line is
        // (spec B4.4). What is under test is unchanged — that the command which was
        // interrupted closes as stopped and never also as finished.
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

    /// Spec B4.5's two halves as the pump sees them: a shell that marks only `A` and `B`,
    /// and a device-query answer the far end never read.
    /// **B5.6: the prompt is spoken again.** A session over a shell that marks all four
    /// boundaries had no way to say what its prompt says — the `A..B` region is excluded
    /// from block content, and `D` closes the block before the next prompt is drawn, so
    /// the working directory and the git branch a listener navigates by were audible
    /// nowhere at all.
    mod the_prompt_a_marked_shell_draws {
        use super::*;

        async fn marked() -> Session {
            Session::of(quick_grace(), ShellMarkers::Full).await
        }

        /// A prompt drawn and then finished, which is what `B` means: the shell has
        /// stopped drawing and is reading a command line.
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

        /// The session's very first prompt, before anything has been run: a listener starts
        /// knowing where they are rather than having to run a command to find out.
        #[tokio::test]
        async fn is_spoken_before_any_command_has_run() {
            let session = marked().await;
            session.emit(prompt(1, PROMPT)).await;
            session.advance_to(1_000).await;

            assert_eq!(prompts(&session), vec![PROMPT.to_owned()]);
        }

        /// **The regression this entry exists for.** After a command ends, the next prompt
        /// is drawn, and it has to be heard: it is where the working directory and the
        /// branch changed.
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

        /// It arrives after the block has closed, which is the order a listener needs:
        /// what happened, then where they are now. This falls out of the byte order — the
        /// shell draws its prompt after `D` — and is asserted so a later change cannot
        /// quietly reverse it.
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

        /// A prompt of nothing but whitespace is not something to read out: some shells
        /// draw across two rows and the first is blank.
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

        /// The prompt drawn, the command line marked, the echo appended to the prompt's
        /// own row — exactly what a real `cmd.exe` puts on the wire.
        fn prompt(row: u64, at: &str) -> Vec<TerminalItem> {
            vec![
                marker(Osc133Marker::PromptStart),
                line(row, at),
                marker(Osc133Marker::CommandStart),
            ]
        }

        /// A block opens where the echo ends, and it is the submission's block: the
        /// heading names what the user typed, and the output is under it.
        #[tokio::test]
        async fn the_echo_opens_the_submissions_block_and_names_it() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("dir").await;
            session.emit(vec![line(1, "dir"), line(2, "one.txt")]).await;
            session.advance_to(1_000).await;

            // Two blocks: the session's first prompt belongs to no command anyone
            // submitted and gets one of its own, exactly as DESIGN says, and the
            // submission gets the one the synthesized `C` opened.
            assert_eq!(session.started().last(), Some(&command));
            assert_eq!(session.headings(), vec![None, Some("dir".to_owned())]);
            assert_eq!(session.output_of(command), "one.txt");
        }

        /// Decision 4, and the reason it amends a pinned answer: a cmd session has no exit
        /// code, so the returning prompt is the only ending a listener gets. It has to be
        /// inside the block it ended, and it has to be spoken.
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

        /// The echo is still excluded from the block's content — DESIGN's echo exclusion,
        /// doing exactly what the markers were injected to let it do.
        #[tokio::test]
        async fn the_echo_is_never_the_blocks_content() {
            let session = cmd().await;
            session.emit(prompt(1, PROMPT)).await;
            let command = session.submit("dir").await;
            session.emit(vec![line(1, "dir"), line(2, "one.txt")]).await;
            session.advance_to(1_000).await;

            assert!(!session.output_of(command).contains("dir"));
        }

        /// The safety constraint: in a shell known not to emit `C`, a line that cannot be
        /// classified must be forwarded rather than dropped. Text on a row that is not the
        /// echo's ends the command line and is spoken.
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

    /// The cancel byte that keeps a submitted line from being concatenated onto input
    /// nobody read (spec B4.5, decisions 6 and 7).
    ///
    /// What is queued in front of the line is not modelled here, because Acter cannot see
    /// it: ConPTY answers a program's cursor-position query itself, so the answer is in the
    /// far end's input queue and never on the wire. What these tests pin is the gate —
    /// which sessions get the byte and which never do — and the real shell proves it works.
    mod the_cancel_ahead_of_a_submission {
        use super::*;

        /// A session over the one shell whose line editor discards on a byte.
        ///
        /// **The shell says so, since B9.5**, where this used to be inferred from the marker
        /// claim: `PromptAndCommandLine` meant `cmd.exe` and said so exactly until POSIX `sh`
        /// became the second shell to claim it.
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

        /// A shell sitting at its prompt: the line goes out behind one escape.
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

        /// **A bare Enter is protected the same way** (spec B4.9, decision 5). Without it
        /// the re-orient gesture returns garbage: the queued answer nobody read is
        /// submitted as a command line, and instead of the prompt the user hears that
        /// something they never typed is not recognized.
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

        /// And it is the same gate, not a new one: an Enter pressed while something is
        /// running is stdin for that program — a REPL, a "press Enter to continue" — so it
        /// goes out as one byte and nothing else.
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

        /// **The gate 22.5 is the precondition for.** A command is running, so the line is
        /// stdin for it — a REPL's answer, a `y` at a `[y/N]` — and an escape reaching a
        /// program that reads raw input is a keypress rather than a line cancel.
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

        /// A second line typed while the first is still unaccounted for is not a line at a
        /// prompt either: the far end has not said what it did with the one before it.
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

        /// And a shell that named no such byte never gets one. Escape clearing the line is
        /// `cmd.exe`'s line editor; a POSIX shell's reader takes it as a meta prefix.
        #[tokio::test]
        async fn a_shell_that_named_no_such_byte_never_gets_one() {
            let session = Session::with_config(quick_grace()).await;
            session.advance_to(300).await;
            session.emit(vec![line(1, PROMPT)]).await;
            session.submit("dir").await;

            assert_eq!(session.written(), "dir\r");
        }

        /// **The case B9.5 made reachable, and the reason this stopped being read off the
        /// marker claim** (measured 2026-08-29 against `docker-desktop`). POSIX `sh` marks its
        /// prompt boundaries and nothing further, exactly as `cmd.exe` does — and an escape
        /// written into it is a keypress, which left busybox running a fragment of the line
        /// behind it and answering `-sh: r-sh: not found`.
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

    /// Saying there is no more input, which arrived with the first shell that had an
    /// answer to give (spec B5.2, decision 5).
    ///
    /// What these tests pin is the *seam*: the session writes whatever the adapter said
    /// and invents nothing, and it says so honestly when the adapter said nothing. Which
    /// bytes are right for PowerShell is measured against a real one in
    /// `acter-transports`' real-shell suite, because that is a fact about a shell rather
    /// than about this service.
    mod end_of_input {
        use super::*;

        /// The bytes are the adapter's, not this service's. A session that had learned one
        /// shell's answer would be right for exactly one shell and silently wrong for the
        /// next, which is the whole reason this is a port method.
        #[tokio::test]
        async fn the_session_writes_whatever_the_shell_said_ends_it() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;

            assert_eq!(session.press(ctrl('d')).await, KeyAck::Applied);
            assert_eq!(session.written(), "stop-this-shell");
        }

        /// **A shell with no measured answer writes nothing at all**, which is the half
        /// worth pinning: guessing at a control byte is how B5.2 found that `0x1a` reaches
        /// PowerShell as caret text and turns the next submission into a command the user
        /// never typed.
        #[tokio::test]
        async fn a_shell_with_no_answer_writes_nothing_and_says_so() {
            let session = Session::with_config(quick_grace()).await;

            assert_eq!(session.press(ctrl('d')).await, KeyAck::NothingToActOn);
            assert_eq!(session.written(), "");
        }

        /// Unlike the interrupt beside it, this never asks whether a command is running: a
        /// shell waiting at its prompt is the ordinary case for ending a session, and that
        /// is precisely the moment when nothing is running.
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

        /// It opens no block and claims no correlation id: a keystroke is not a command the
        /// user composed, and giving it a heading would put a line in the buffer that
        /// nobody typed.
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

        /// The far end is never interrupted by it. Two keystrokes, two intents, and a
        /// session that confused them would stop a running command when the user asked to
        /// leave.
        #[tokio::test]
        async fn it_is_not_an_interrupt() {
            let session = Session::over(quick_grace(), ending_with(b"stop-this-shell")).await;
            session.press(ctrl('d')).await;

            assert_eq!(session.interrupts(), 0);
        }
    }

    /// The session is set up after it is established (spec B9.5).
    mod the_setup_is_sent_once_the_far_end_speaks {
        use super::*;

        /// Short enough to read in an assertion, and shaped like the real one: a statement
        /// that marks its own output, then the assignment that does the work.
        const SETUP: &str = "printf mark; PROMPT_COMMAND=__acter_prompt";

        async fn set_up() -> Session {
            Session::over(quick_grace(), set_up_with(SETUP)).await
        }

        /// **Not at session start** (spec B9.5, decision 4). Bytes written before the shell
        /// has read them are an unmeasured race, and they are the launch-time injection
        /// wearing a different hat.
        #[tokio::test]
        async fn nothing_is_written_before_the_far_end_has_said_anything() {
            let session = set_up().await;

            assert_eq!(session.written(), "");
        }

        /// **The trigger is the far end speaking**, which is the same fact that makes a
        /// session "connected" at all (spec A9, decision 3) — so the machinery already
        /// existed and this entry did not invent a second one.
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

        /// Once, however much the far end goes on to say. A second copy would be a second
        /// command in the buffer that nobody typed.
        #[tokio::test]
        async fn it_is_sent_once_however_often_the_far_end_speaks() {
            let session = set_up().await;

            session.emit(vec![line(1, PROMPT)]).await;
            session.emit(vec![line(2, "more")]).await;
            session.emit(vec![line(3, "and more")]).await;

            assert_eq!(session.written(), format!("{SETUP}\r"));
        }

        /// **A far end with nothing measured for it has nothing run in it**, which is the
        /// state a shell nobody has written a setup for is in, and the state a connection
        /// whose checkbox was unticked is in. Both reach the session as the same absence.
        #[tokio::test]
        async fn a_far_end_with_no_setup_has_nothing_written_into_it() {
            let session = Session::with_config(quick_grace()).await;

            session.emit(vec![line(1, PROMPT)]).await;

            assert_eq!(session.written(), "");
        }

        /// **The block opens, is headed by the command verbatim, and closes with a real exit
        /// code** (spec B9.5, decision 3). The far end echoes the line, prints the `C` the
        /// setup asks for, and the next prompt carries the `D` — which is the whole cycle,
        /// and the thing that stops the session reporting "running" from the moment it
        /// connects.
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

        /// **Nothing is spoken about it, and that is what makes disclosure affordable.**
        /// Opening a block says nothing, a successful setup prints nothing, and since A6
        /// decision 2 a successful command's exit code is not on the wire at all — so the
        /// listener hears the connection sentence and then the prompt, with the command
        /// sitting in the buffer for anyone who goes looking.
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

        /// **The empty heading, pinned** (spec B9.5, decision 3). A far end that rewrites the
        /// line instead of echoing it leaves the block to be opened by the `C` the setup
        /// printed — and `claim` used to drop the line it came from, so the block reached the
        /// buffer with nothing on it at all. The frontend cannot fill this one in for itself:
        /// there is no submit ack, because no frontend submitted it.
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

        /// **And a line the *user* typed is still left alone**, which is B6.1's decision 1 and
        /// is not weakened by the rule above: the frontend already put the typed text on that
        /// block, so a heading from here that is not the shell's own echo would overwrite it
        /// with a guess a drifted id could attach to the wrong block.
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

        /// **Ctrl+C immediately after connecting has nothing to act on**, because the setup
        /// has opened and closed. The failure this guards is a listener told they stopped a
        /// command they never ran.
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
