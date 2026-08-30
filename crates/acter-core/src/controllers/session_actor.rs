//! Controller: the per-session loop. Owns one session's pacing state, session state and
//! buffers; turns domain facts into protocol events by asking the pacing policy what to
//! do and the [`Clock`] when to be asked again. Deleting it would lose connectivity, not
//! business behavior — every decision it makes belongs to `policies::autoread`.
//!
//! Two paths, two cadences (DESIGN, buffer and speech are separate paths). Rendering is
//! unconditional and runs on a short coalescing tick; speech runs on the pacing windows
//! and is the only path the policy governs. The actor emits the rendering event covering
//! a span before any announcement about it, which is what makes "never announce text that
//! is not already in the buffer" true on an ordered channel without correlating anything.
//!
//! The decision logic is synchronous methods; [`SessionActor::run`] is a thin async loop
//! over them. That is deliberate: tests drive the methods directly and read back the
//! wake-ups that were requested, so the scheduling contract is asserted without a runtime,
//! a sleep, or a real clock.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::entities::{ReadMode, UnspokenText};
use crate::policies::{PacingAction, measure, on_command_end, on_output, on_wake};
use crate::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, Mode, PacingConfig,
    PacingState, SessionEvent, SessionState, Timer,
};

/// A domain fact the actor is told about. Deliberately not bytes: extraction, OSC 133
/// recognition and PTY reading belong to B2/B3/B4, which later become the things that
/// feed this channel instead of a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInput {
    /// A command's output region has begun.
    ///
    /// `command_line` is the echo the service read for this block, carried through
    /// unchanged: what the shell said it is running, or `None` when it did not say
    /// (spec B6.1, decision 1).
    CommandStarted {
        command_id: CommandId,
        command_line: Option<String>,
    },
    /// Text arrived for the running command. An empty chunk is not output and must not
    /// move the quiescence deadline (B1.1), which the policy enforces.
    Output {
        text: String,
    },
    /// The running command ended on its own.
    /// The shell drew a prompt. Carried through the actor untouched: what it says is the
    /// shell's business, and whether it is spoken is the frontend's (spec B5.6).
    PromptDrawn {
        text: String,
    },
    /// The transport's connection state changed: the far end spoke for the first time, or
    /// it went away. Passed through untouched — what a window does with it is the
    /// frontend's (spec A9).
    Connection {
        state: ConnectionState,
    },
    CommandEnded {
        command_id: CommandId,
        exit_code: ExitCode,
    },
    /// The running command was stopped rather than finishing.
    ///
    /// Only the service can tell the two apart, and not from the exit code: a block
    /// closing with no code is either a bare `D` or a prompt reappearing mid-block (B2),
    /// so what distinguishes them is that the service had asked the transport to
    /// interrupt. Left to the exit code, a stopped command would be announced as
    /// "finished, exit code 0" — the wrong announcement A3.1 decision 4 created
    /// `CommandInterrupted` to avoid (spec B6, decision 8).
    CommandInterrupted {
        command_id: CommandId,
    },
    /// Shell-integration markers were observed: command boundaries are trustworthy.
    /// Resolves the session, and recovers one already flagged unintegrated.
    MarkersObserved,
    /// The startup grace period elapsed. Resolves a session that has seen no markers to
    /// unintegrated; a session already resolved either way is unaffected.
    GracePeriodExpired,
    /// Follow mode was toggled. An explicit override: every chunk is read on arrival.
    FollowMode(bool),
    /// Acter is talking to itself: everything arriving until this turns off is rendered and
    /// none of it is spoken (roadmap 23.12).
    ///
    /// **From the instant Acter writes its own line to the instant that line's block
    /// closes**, nothing in the session is the user's to hear — not the shell's echo of a
    /// command they never typed, not the prompt drawn twice a tenth of a second apart, not
    /// whatever a far end prints before its first prompt. It was measured being read aloud
    /// two different ways on two different far ends: busybox redrawing a wrapped line in an
    /// order the echo matcher cannot recognise, and `sshd` printing `Last login: ...` before
    /// the prompt so that the row the echo was expected on is the banner's.
    ///
    /// **It is one line of policy rather than a better matcher**, and that is the point: it
    /// does not depend on recognising an echo, so it is indifferent to how any far end
    /// redraws a wrapped line. Nothing is withheld — [`ReadMode::Quiet`] already means
    /// "accumulates silently in the buffer", so every byte stays where the disclosure the
    /// Connect dialog is about can be read back.
    SelfTalk(bool),
    /// A program entered or left the alternate screen.
    AltScreenEntered,
    AltScreenLeft,
}

/// What the actor wants done with one of its two timers after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wake {
    /// Leave whatever is armed alone.
    #[default]
    Unchanged,
    /// Nothing more is pending; drop the timer.
    Clear,
    /// Re-arm for this long from now.
    After(Duration),
}

/// The timer changes one step asked for. Read by [`SessionActor::run`], and by tests
/// asserting that a wake was armed for exactly the duration the policy returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Requests {
    pub render: Wake,
    pub pacing: Wake,
}

/// One command's in-flight state. Retired when the command ends, so nothing leaks into
/// the next one — a fresh `PacingState` is exactly what B1 expects per command.
#[derive(Debug)]
struct ActiveCommand {
    id: CommandId,
    started_at: Duration,
    pacing: PacingState,
    /// Text not yet sent to the buffer. Flushed on the coalescing tick, and always
    /// before an announcement that covers it.
    unrendered: String,
    unspoken: UnspokenText,
    render_armed: bool,
}

impl ActiveCommand {
    fn new(id: CommandId, started_at: Duration) -> Self {
        Self {
            id,
            started_at,
            pacing: PacingState::default(),
            unrendered: String::new(),
            unspoken: UnspokenText::default(),
            render_armed: false,
        }
    }
}

/// One session's actor.
pub struct SessionActor {
    config: PacingConfig,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EventSink>,
    session: SessionState,
    follow_mode: bool,
    /// Whether Acter's own line is what the far end is currently talking about, in which
    /// case nothing coming back is spoken (roadmap 23.12). See [`SessionInput::SelfTalk`].
    self_talk: bool,
    active: Option<ActiveCommand>,
    requests: Requests,
}

impl SessionActor {
    pub fn new(config: PacingConfig, clock: Arc<dyn Clock>, sink: Arc<dyn EventSink>) -> Self {
        Self {
            config,
            clock,
            sink,
            // Phase 1 only ever renders non-interactively (A2); the mode toggle is not an
            // actor input yet.
            session: SessionState::new(Mode::NonInteractive),
            follow_mode: false,
            self_talk: false,
            active: None,
            requests: Requests::default(),
        }
    }

    /// Runs until the input channel closes. Thin by design: every decision is in the
    /// synchronous methods below.
    ///
    /// The channel is unbounded because the actor is its single consumer and never waits
    /// on anything itself, so nothing is gained by making its producer wait — while
    /// dropping one of these is losing a domain fact, and for `Output` that is losing
    /// text, which is this product's cardinal defect. Back-pressure belongs one seam
    /// lower, on the bounded read channel between the transport and the pump.
    pub async fn run(mut self, mut inputs: mpsc::UnboundedReceiver<SessionInput>) {
        let mut render_timer: Option<Timer> = None;
        let mut pacing_timer: Option<Timer> = None;

        loop {
            // Resolved before any state is touched, so no timer future is alive while
            // the step below mutates the actor.
            let woke = tokio::select! {
                input = inputs.recv() => match input {
                    Some(input) => Woke::Input(input),
                    None => break,
                },
                () = fire(&mut render_timer) => Woke::Render,
                () = fire(&mut pacing_timer) => Woke::Pacing,
            };

            match woke {
                Woke::Input(input) => self.handle(input),
                Woke::Render => {
                    render_timer = None;
                    self.wake_render();
                }
                Woke::Pacing => {
                    pacing_timer = None;
                    self.wake_pacing();
                }
            }

            let requests = self.take_requests();
            apply(&mut render_timer, requests.render, self.clock.as_ref());
            apply(&mut pacing_timer, requests.pacing, self.clock.as_ref());
        }
    }

    /// The timer changes the last step asked for, cleared as they are read.
    pub fn take_requests(&mut self) -> Requests {
        std::mem::take(&mut self.requests)
    }

    /// One domain fact.
    pub fn handle(&mut self, input: SessionInput) {
        match input {
            SessionInput::CommandStarted {
                command_id,
                command_line,
            } => self.command_started(command_id, command_line),
            SessionInput::Output { text } => self.output(&text),
            SessionInput::CommandEnded {
                command_id,
                exit_code,
            } => self.command_ended(command_id, exit_code),
            SessionInput::CommandInterrupted { command_id } => self.command_interrupted(command_id),
            SessionInput::Connection { state } => {
                self.sink.send(SessionEvent::ConnectionChanged { state })
            }
            // Passed straight through, and deliberately not routed past the pacing policy:
            // a prompt is neither output nor a verdict, nothing about it accumulates, and
            // it is short by construction — the babble guard exists for a shell that will
            // not stop talking, which is not what drawing a prompt is (spec B5.6).
            SessionInput::PromptDrawn { text } => {
                self.sink.send(SessionEvent::PromptDrawn { text })
            }
            SessionInput::MarkersObserved => self.session = self.session.markers_observed(),
            SessionInput::GracePeriodExpired => {
                let next = self.session.grace_period_expired();
                if next != self.session {
                    self.session = next;
                    self.sink.send(SessionEvent::IntegrationUnavailable);
                }
            }
            SessionInput::FollowMode(on) => self.follow_mode = on,
            SessionInput::SelfTalk(on) => self.self_talk = on,
            SessionInput::AltScreenEntered => {
                let next = self.session.alt_screen_entered();
                if next != self.session {
                    self.session = next;
                    self.sink.send(SessionEvent::AltScreenEntered);
                }
            }
            SessionInput::AltScreenLeft => {
                let next = self.session.alt_screen_left();
                if next != self.session {
                    self.session = next;
                    self.sink.send(SessionEvent::AltScreenLeft);
                }
            }
        }
    }

    /// The coalescing tick fired: everything accumulated reaches the buffer.
    pub fn wake_render(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.render_armed = false;
        }
        self.flush_render();
    }

    /// A pacing deadline fired: quiescence or patience, as the policy decides.
    pub fn wake_pacing(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let at = self.clock.now().saturating_sub(active.started_at);
        let (pacing, outcome) = on_wake(active.pacing, &self.config, active.unspoken.size(), at);
        active.pacing = pacing;
        self.apply(outcome.action);
        self.requests.pacing = wake_from(outcome.wake_after);
    }

    /// The echo passes straight through: reading it is the pump's, and the actor decides
    /// nothing about it — it owns the event, not the correlation.
    fn command_started(&mut self, command_id: CommandId, command_line: Option<String>) {
        self.active = Some(ActiveCommand::new(command_id, self.clock.now()));
        self.sink.send(SessionEvent::CommandStarted {
            command_id,
            command_line,
        });
    }

    fn output(&mut self, text: &str) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        // Rendering is unconditional: the buffer loads whenever content arrives, whatever
        // speech later decides (DESIGN). The tick is a fixed cadence, not a debounce —
        // re-arming it on every chunk would starve rendering under continuous output.
        active.unrendered.push_str(text);
        if !active.unrendered.is_empty() && !active.render_armed {
            active.render_armed = true;
            self.requests.render = Wake::After(self.config.render_tick);
        }

        active.unspoken.push(text, &self.config);
        let at = self.clock.now().saturating_sub(active.started_at);
        let (pacing, outcome) = on_output(
            active.pacing,
            &self.config,
            measure(text),
            at,
            self.follow_mode,
        );
        active.pacing = pacing;
        self.apply(outcome.action);
        self.requests.pacing = wake_from(outcome.wake_after);
    }

    fn command_ended(&mut self, command_id: CommandId, exit_code: ExitCode) {
        if !self.close(SessionEvent::CommandFinished { command_id }) {
            return;
        }
        if exit_code.0 != 0 {
            self.announce(Announcement::Failed { exit_code });
        }
        self.retire();
    }

    /// The same close, with the terminal event that says the user stopped it. No
    /// `Failed`: the exit code of a process the user stopped carries nothing worth
    /// announcing, and the frontend already has one thing to say about a stop.
    fn command_interrupted(&mut self, command_id: CommandId) {
        if !self.close(SessionEvent::CommandInterrupted { command_id }) {
            return;
        }
        self.retire();
    }

    /// What both endings share, in the order a listener needs: the policy's last word on
    /// the remainder, everything reaching the buffer before anything is said about it,
    /// whatever there was to say — and only then the terminal event.
    ///
    /// **The last word comes before the ending, and that ordering is load-bearing.** The
    /// remainder is text that arrived *during* this command, so an announcement about it
    /// describes something that happened before the command ended and belongs before the
    /// event that says so; this is the same rule A6 decision 2 applied to `Failed`, which
    /// follows the output it is a verdict on. Emitting the ending first also silently
    /// broke the completion beep, which the frontend fires on the ending for any command
    /// a `TooBig` had armed — the arming verdict arrived one event too late, every time,
    /// so no too-big command ever beeped (found in A3.2's manual pass).
    ///
    /// `false` when no command was running, which is a fact about the far end rather than
    /// an error — a `D` with no open block is DESIGN's reliability case 3.
    fn close(&mut self, terminal: SessionEvent) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let (pacing, outcome) = on_command_end(active.pacing, &self.config, active.unspoken.size());
        active.pacing = pacing;
        self.flush_render();
        self.apply(outcome.action);
        self.sink.send(terminal);
        true
    }

    /// The command is over: nothing accumulates for it and neither timer has anything
    /// left to wake for.
    fn retire(&mut self) {
        self.active = None;
        self.requests.render = Wake::Clear;
        self.requests.pacing = Wake::Clear;
    }

    /// Turns one policy decision into events. Every branch that speaks flushes the
    /// rendering path first — the whole mechanism behind DESIGN's invariant.
    fn apply(&mut self, action: PacingAction) {
        match self.aside(action) {
            PacingAction::None => {}
            PacingAction::Flush(mode) => {
                self.flush_render();
                let Some(active) = self.active.as_mut() else {
                    return;
                };
                let (text, size) = active.unspoken.take();
                match mode {
                    // Rendered, not spoken: the babble guard went quiet, so the buffer
                    // keeps up and the listener is left alone.
                    ReadMode::Quiet => {}
                    // Read aloud, in every session (spec B4.4). DESIGN's reliability case
                    // 2 used to say a session with no integration degrades to no
                    // auto-read, and that argument was written when the alternative was
                    // *guessing* — a prompt matched by its shape, output attributed by
                    // inference. Repeating text that genuinely arrived is not a guess; it
                    // is the same category as autoread everywhere else, which is the
                    // distinction B4.1 turned on when `command stopped` came out. A
                    // degraded session that renders everything and speaks none of it is
                    // silent to the only user this product has.
                    //
                    // The rest of case 2 stands: no exit code, no verdict, and the
                    // patience announcement still fires.
                    ReadMode::Auto => {
                        if let Some(text) = text {
                            self.announce(Announcement::ReadAloud { text });
                        }
                    }
                    ReadMode::TooBig => self.announce(Announcement::TooBig {
                        lines: size.lines.try_into().unwrap_or(u32::MAX),
                    }),
                }
            }
            PacingAction::StillRunning => self.announce(Announcement::StillRunning),
            PacingAction::OutputContinues => self.announce(Announcement::OutputContinues),
        }
    }

    /// What one policy decision becomes while Acter is talking to itself (roadmap 23.12).
    ///
    /// **Applied here rather than in the policy**, and that is where it belongs: the pacing
    /// policy answers questions about size and repetition, and this is not one — it is a fact
    /// about *whose* command is running, which the policy has no business knowing. Everything
    /// still flushes, so the buffer keeps every byte; nothing is announced, including the
    /// patience and babble announcements, which would otherwise report on the progress of a
    /// command the user did not run.
    fn aside(&self, action: PacingAction) -> PacingAction {
        if !self.self_talk {
            return action;
        }
        match action {
            PacingAction::Flush(_) => PacingAction::Flush(ReadMode::Quiet),
            PacingAction::None | PacingAction::StillRunning | PacingAction::OutputContinues => {
                PacingAction::None
            }
        }
    }

    fn flush_render(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.unrendered.is_empty() {
            return;
        }
        let text = std::mem::take(&mut active.unrendered);
        let command_id = active.id;
        // Render-only: the verdict rides an `Announce` (A6 retired the field that used
        // to duplicate it here).
        self.sink.send(SessionEvent::Output { command_id, text });
    }

    fn announce(&self, announcement: Announcement) {
        if let Some(active) = self.active.as_ref() {
            self.sink.send(SessionEvent::Announce {
                command_id: active.id,
                announcement,
            });
        }
    }
}

enum Woke {
    Input(SessionInput),
    Render,
    Pacing,
}

fn wake_from(wake_after: Option<Duration>) -> Wake {
    match wake_after {
        Some(after) => Wake::After(after),
        None => Wake::Clear,
    }
}

fn apply(timer: &mut Option<Timer>, wake: Wake, clock: &dyn Clock) {
    match wake {
        Wake::Unchanged => {}
        Wake::Clear => *timer = None,
        Wake::After(after) => *timer = Some(clock.timer(after)),
    }
}

/// Awaits an armed timer, or waits forever when none is — so an unarmed branch simply
/// never wins the select rather than needing a precondition.
async fn fire(timer: &mut Option<Timer>) {
    match timer {
        Some(timer) => timer.await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::sync::oneshot;

    use super::*;

    /// Time moves only when a test says so, and an armed timer fires only when its
    /// deadline is reached. Nothing sleeps and nothing reads the real clock.
    #[derive(Default)]
    struct FakeClock {
        now: Mutex<Duration>,
        armed: Mutex<Vec<(Duration, oneshot::Sender<()>)>>,
    }

    impl FakeClock {
        /// Moves to `at` and fires every timer due by then.
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

    impl Recorder {
        fn events(&self) -> Vec<SessionEvent> {
            self.0.lock().expect("recorder poisoned").clone()
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
    }

    fn actor() -> (SessionActor, Arc<FakeClock>, Arc<Recorder>) {
        let clock = Arc::new(FakeClock::default());
        let sink = Arc::new(Recorder::default());
        let actor = SessionActor::new(PacingConfig::default(), clock.clone(), sink.clone());
        (actor, clock, sink)
    }

    fn started(actor: &mut SessionActor) {
        actor.handle(SessionInput::CommandStarted {
            command_id: CommandId(1),
            command_line: None,
        });
        let _ = actor.take_requests();
    }

    fn output(actor: &mut SessionActor, text: &str) -> Requests {
        actor.handle(SessionInput::Output {
            text: text.to_owned(),
        });
        actor.take_requests()
    }

    fn ended(actor: &mut SessionActor, command_id: u32, exit_code: i32) {
        actor.handle(SessionInput::CommandEnded {
            command_id: CommandId(command_id),
            exit_code: ExitCode(exit_code),
        });
    }

    /// The actor is a conduit for the echo and nothing more: reading it is the service's,
    /// and nothing here inspects, trims or substitutes it (spec B6.1, decision 1).
    #[test]
    fn the_echoed_command_line_reaches_the_frontend_unchanged() {
        let (mut actor, _clock, sink) = actor();

        actor.handle(SessionInput::CommandStarted {
            command_id: CommandId(1),
            command_line: Some("git status".to_owned()),
        });

        assert_eq!(
            sink.events(),
            vec![SessionEvent::CommandStarted {
                command_id: CommandId(1),
                command_line: Some("git status".to_owned()),
            }]
        );
    }

    // --- The scheduling contract ------------------------------------------------

    #[test]
    fn the_wake_armed_is_exactly_what_the_policy_returned() {
        let config = PacingConfig::default();
        let (mut actor, _clock, _sink) = actor();
        started(&mut actor);

        let requests = output(&mut actor, "hello\n");
        assert_eq!(requests.pacing, Wake::After(config.quiescence));
        assert_eq!(requests.render, Wake::After(config.render_tick));
    }

    #[test]
    fn an_empty_chunk_does_not_push_the_deadline_out() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);
        output(&mut actor, "Password:");

        // A repaint at 400ms carries no text, so the 500ms deadline stands: the wake is
        // what remains of it, not a fresh window (B1.1, from the caller side).
        clock.advance_to(Duration::from_millis(400));
        let requests = output(&mut actor, "");
        assert_eq!(requests.pacing, Wake::After(Duration::from_millis(100)));

        // And the prompt is still read on time.
        clock.advance_to(config.quiescence);
        actor.wake_pacing();
        assert_eq!(
            sink.announcements(),
            vec![Announcement::ReadAloud {
                text: "Password:".to_owned()
            }]
        );
    }

    #[test]
    fn output_inside_the_window_extends_the_deadline() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);
        output(&mut actor, "first\n");

        clock.advance_to(Duration::from_millis(300));
        output(&mut actor, "second\n");

        clock.advance_to(config.quiescence);
        actor.wake_pacing();
        assert!(sink.announcements().is_empty(), "only 200ms of silence");

        clock.advance_to(Duration::from_millis(800));
        actor.wake_pacing();
        assert_eq!(
            sink.announcements(),
            vec![Announcement::ReadAloud {
                text: "first\nsecond\n".to_owned()
            }],
            "one chunk, both spans"
        );
    }

    // --- Two paths --------------------------------------------------------------

    #[test]
    fn rendering_happens_on_the_tick_whatever_speech_decides() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        output(&mut actor, "text\n");

        assert_eq!(sink.rendered(), "", "nothing rendered before the tick");
        actor.wake_render();
        assert_eq!(
            sink.events().last(),
            Some(&SessionEvent::Output {
                command_id: CommandId(1),
                text: "text\n".to_owned(),
            }),
            "rendering carries no verdict: the verdict rides an Announce"
        );
    }

    #[test]
    fn a_tripped_guard_keeps_rendering_and_stops_announcing() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);

        // Chunks separated by a full quiescent gap: three are read, the fourth trips the
        // guard, the fifth is silent (B1.1).
        let mut at = Duration::ZERO;
        for _ in 0..5 {
            output(&mut actor, "chatty line\n");
            at += config.quiescence;
            clock.advance_to(at);
            actor.wake_pacing();
        }

        let announcements = sink.announcements();
        assert_eq!(announcements.len(), config.babble_limit as usize + 1);
        assert_eq!(announcements.last(), Some(&Announcement::OutputContinues));
        assert_eq!(
            sink.rendered(),
            "chatty line\n".repeat(5),
            "going quiet never withholds text"
        );
    }

    #[test]
    fn every_announcement_is_preceded_by_the_render_that_covers_it() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);

        // The render tick is deliberately never fired: the announcement path must flush
        // the buffer itself rather than rely on the tick having got there first.
        let mut at = Duration::ZERO;
        for chunk in ["one\n", "two\n", "three\n"] {
            output(&mut actor, chunk);
            at += config.quiescence;
            clock.advance_to(at);
            actor.wake_pacing();
        }
        ended(&mut actor, 1, 0);

        let mut rendered = String::new();
        for event in sink.events() {
            match event {
                SessionEvent::Output { text, .. } => rendered.push_str(&text),
                SessionEvent::Announce {
                    announcement: Announcement::ReadAloud { text },
                    ..
                } => assert!(
                    rendered.contains(&text),
                    "announced {text:?} before the buffer had it; rendered so far {rendered:?}"
                ),
                _ => {}
            }
        }
        assert_eq!(rendered, "one\ntwo\nthree\n");
    }

    /// **Acter's own line is not the user's to hear** (roadmap 23.12). Everything the far
    /// end says while it is running reaches the buffer and none of it is announced.
    #[test]
    fn while_acter_talks_to_itself_the_buffer_keeps_everything_and_nobody_is_told() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::SelfTalk(true));
        started(&mut actor);

        output(
            &mut actor,
            "printf mark; PROMPT_COMMAND=__acter_prompt
",
        );
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.rendered(),
            "printf mark; PROMPT_COMMAND=__acter_prompt
",
            "every byte stays reviewable, which is what the disclosure rests on"
        );
        assert_eq!(
            sink.announcements(),
            vec![],
            "and none of it is read aloud: {:?}",
            sink.announcements()
        );
    }

    /// **Including the announcements that are about a command rather than its text.** A
    /// flood normally says how big it was; a flood of Acter's own making says nothing,
    /// because there is nobody it would be telling about their own command.
    #[test]
    fn a_flood_of_acters_own_making_says_nothing_at_all() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::SelfTalk(true));
        started(&mut actor);

        for _ in 0..500 {
            output(
                &mut actor, "y
",
            );
        }
        clock.advance_to(config.quiescence);
        actor.wake_pacing();
        clock.advance_to(config.quiescence + config.patience);
        actor.wake_pacing();

        assert_eq!(
            sink.rendered(),
            "y
"
            .repeat(500),
            "all of it is reviewable"
        );
        assert_eq!(
            sink.announcements(),
            vec![],
            "no size, no patience, no babble: {:?}",
            sink.announcements()
        );
    }

    /// **And the window closes.** What arrives after it is the user's session again, spoken
    /// exactly as it would have been — the state is a window and not a mode.
    #[test]
    fn what_arrives_after_the_window_closes_is_read_aloud_again() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::SelfTalk(true));
        started(&mut actor);
        output(
            &mut actor,
            "acter's own
",
        );
        actor.handle(SessionInput::SelfTalk(false));

        output(
            &mut actor,
            "the user's
",
        );
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![Announcement::ReadAloud {
                text: "acter's own
the user's
"
                .to_owned()
            }],
            "what was accumulated while the window was open is spoken with what follows              it, because closing the window is not a reason to throw text away: {:?}",
            sink.announcements()
        );
    }

    #[test]
    fn a_flood_announces_by_size_and_never_holds_the_bytes() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);

        for _ in 0..500 {
            output(&mut actor, "y\n");
        }
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![Announcement::TooBig { lines: 500 }],
            "counted exactly, without keeping the text to count it"
        );
        assert_eq!(
            sink.rendered(),
            "y\n".repeat(500),
            "all of it is reviewable"
        );
    }

    // --- Lifecycle --------------------------------------------------------------

    #[test]
    fn a_second_command_starts_from_a_fresh_pacing_state() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();

        started(&mut actor);
        let mut at = Duration::ZERO;
        for _ in 0..5 {
            output(&mut actor, "line\n");
            at += config.quiescence;
            clock.advance_to(at);
            actor.wake_pacing();
        }
        ended(&mut actor, 1, 0);

        actor.handle(SessionInput::CommandStarted {
            command_id: CommandId(2),
            command_line: None,
        });
        let _ = actor.take_requests();
        output(&mut actor, "fresh\n");
        at += config.quiescence;
        clock.advance_to(at);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements().last(),
            Some(&Announcement::ReadAloud {
                text: "fresh\n".to_owned()
            }),
            "the tripped guard did not leak across commands"
        );
    }

    #[test]
    fn ending_a_command_clears_both_timers() {
        let (mut actor, _clock, _sink) = actor();
        started(&mut actor);
        output(&mut actor, "text\n");

        ended(&mut actor, 1, 0);
        assert_eq!(
            actor.take_requests(),
            Requests {
                render: Wake::Clear,
                pacing: Wake::Clear,
            }
        );
    }

    #[test]
    fn a_failing_command_announces_its_exit_code_after_its_output() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        output(&mut actor, "boom\n");

        ended(&mut actor, 1, 2);
        assert_eq!(
            sink.announcements(),
            vec![
                Announcement::ReadAloud {
                    text: "boom\n".to_owned()
                },
                Announcement::Failed {
                    exit_code: ExitCode(2)
                },
            ]
        );
    }

    #[test]
    fn follow_mode_reads_each_chunk_on_arrival() {
        let (mut actor, _clock, sink) = actor();
        actor.handle(SessionInput::FollowMode(true));
        started(&mut actor);

        for chunk in ["one\n", "two\n", "three\n"] {
            let requests = output(&mut actor, chunk);
            assert_eq!(
                requests.pacing,
                Wake::Clear,
                "nothing accumulates, so there is nothing to wake for"
            );
        }
        assert_eq!(
            sink.announcements().len(),
            3,
            "follow mode bypasses the babble guard"
        );
    }

    // --- Integration ------------------------------------------------------------

    #[test]
    fn the_grace_period_expiring_with_no_markers_says_so_once() {
        let (mut actor, _clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        actor.handle(SessionInput::GracePeriodExpired);

        assert_eq!(
            sink.events(),
            vec![SessionEvent::IntegrationUnavailable],
            "a session is flagged unintegrated once, not once per expiry"
        );
    }

    #[test]
    fn a_marker_before_the_grace_period_expires_keeps_the_session_quiet() {
        let (mut actor, _clock, sink) = actor();
        actor.handle(SessionInput::MarkersObserved);
        actor.handle(SessionInput::GracePeriodExpired);

        assert!(
            sink.events().is_empty(),
            "an integrated session announces nothing: {:?}",
            sink.events()
        );
    }

    /// DESIGN decision 8's recovery, from the actor's side: a late marker upgrades the
    /// session and auto-read comes back, with nothing said about it.
    #[test]
    fn a_late_marker_recovers_the_session_and_restores_auto_read() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        started(&mut actor);

        output(
            &mut actor,
            "degraded
",
        );
        clock.advance_to(config.quiescence);
        actor.wake_pacing();
        // **Inverted by B4.4, deliberately kept rather than deleted.** This asserted that
        // an unintegrated session reads nothing aloud, which was DESIGN's reliability case
        // 2 before B4.4 amended it: text that genuinely arrived is evidence, not the guess
        // the no-auto-read rule was protecting against, and a session that renders
        // everything while speaking none of it is silent to the only user this product
        // has.
        assert_eq!(
            sink.announcements(),
            vec![Announcement::ReadAloud {
                text: "degraded\n".to_owned()
            }],
            "a session with no integration reads its output aloud"
        );

        actor.handle(SessionInput::MarkersObserved);
        output(
            &mut actor,
            "recovered
",
        );
        clock.advance_to(config.quiescence * 2);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![
                Announcement::ReadAloud {
                    text: "degraded\n".to_owned()
                },
                Announcement::ReadAloud {
                    text: "recovered\n".to_owned()
                }
            ],
            "and goes on reading aloud once markers recover it"
        );
        assert_eq!(
            sink.events()
                .iter()
                .filter(|event| **event == SessionEvent::IntegrationUnavailable)
                .count(),
            1,
            "recovery is silent"
        );
    }

    /// The degraded session gets status and never content: nothing is read aloud, and
    /// the buffer, the patience window and the size announcement all keep working.
    #[test]
    fn an_unintegrated_session_renders_everything_and_reads_nothing() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        started(&mut actor);

        for _ in 0..500 {
            output(
                &mut actor, "y
",
            );
        }
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![Announcement::TooBig { lines: 500 }],
            "size is status, not content: it is still announced"
        );
        assert_eq!(
            sink.rendered(),
            "y
"
            .repeat(500),
            "every line is reviewable, which is the whole of case 1"
        );
    }

    #[test]
    fn an_unintegrated_session_still_announces_patience() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        started(&mut actor);

        // Output flowing with no quiescent gap for the whole patience window.
        let mut at = Duration::ZERO;
        while at < config.patience {
            at += config.quiescence / 2;
            clock.advance_to(at);
            output(
                &mut actor, "working
",
            );
            actor.wake_pacing();
        }

        assert!(
            sink.announcements().contains(&Announcement::StillRunning),
            "case 2 degrades to case 1, which is the patience announcement: {:?}",
            sink.announcements()
        );
        assert!(
            !sink
                .announcements()
                .iter()
                .any(|announcement| matches!(announcement, Announcement::ReadAloud { .. })),
            "and case 1 is no auto-read"
        );
    }

    // --- The order an ending is reported in ---------------------------------------

    /// The defect this pins was found by ear, not by a test: **no too-big command ever
    /// beeped.** The frontend fires the completion beep on the ending event for any
    /// command a `TooBig` armed, and the ending used to be emitted before the verdict
    /// about the remainder — so the arming always arrived one event too late and the
    /// beep never fired at all.
    ///
    /// The rule the fix restores is A6 decision 2's, applied to every last word rather
    /// than only to `Failed`: an announcement about text that arrived *during* a command
    /// describes something that happened before the command ended, so it is emitted
    /// before the event saying it ended.
    #[test]
    fn the_last_word_on_a_command_is_said_before_the_event_that_ends_it() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        // Comfortably past the auto-read threshold, so the remainder ends as `TooBig`.
        for line in 0..40 {
            output(&mut actor, &format!("line {line}\n"));
        }

        ended(&mut actor, 1, 0);

        let events = sink.events();
        let verdict = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    SessionEvent::Announce {
                        announcement: Announcement::TooBig { .. },
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("the remainder was announced at all: {events:?}"));
        let ending = events
            .iter()
            .position(|event| matches!(event, SessionEvent::CommandFinished { .. }))
            .expect("the command ended");

        assert!(
            verdict < ending,
            "the verdict about the remainder precedes the ending, or the beep it arms \
             fires against a command that has not been armed yet: {events:?}"
        );
    }

    /// The same ordering on the other ending, which is what a listener hears after
    /// pressing Ctrl+C: the output that had accumulated, and then `command stopped`.
    /// Reversed, the session announces a verdict about text it has not read out yet.
    #[test]
    fn a_stop_reads_the_accumulated_output_before_saying_it_stopped() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        output(&mut actor, "one last line\n");

        actor.handle(SessionInput::CommandInterrupted {
            command_id: CommandId(1),
        });

        let events = sink.events();
        let spoken = events
            .iter()
            .position(|event| matches!(event, SessionEvent::Announce { .. }))
            .unwrap_or_else(|| panic!("the remainder was announced: {events:?}"));
        let stopped = events
            .iter()
            .position(|event| matches!(event, SessionEvent::CommandInterrupted { .. }))
            .expect("the stop is reported");

        assert!(
            spoken < stopped,
            "the accumulated output is spoken before the stop is: {events:?}"
        );
    }

    // --- Stopping ---------------------------------------------------------------

    #[test]
    fn an_interrupted_command_is_reported_as_stopped_and_never_as_failed() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        output(
            &mut actor,
            "partial output
",
        );

        actor.handle(SessionInput::CommandInterrupted {
            command_id: CommandId(1),
        });

        assert!(
            sink.events().contains(&SessionEvent::CommandInterrupted {
                command_id: CommandId(1),
            }),
            "the stop is reported: {:?}",
            sink.events()
        );
        assert!(
            !sink
                .events()
                .iter()
                .any(|event| matches!(event, SessionEvent::CommandFinished { .. })),
            "and never also as finished"
        );
        assert!(
            !sink
                .announcements()
                .iter()
                .any(|announcement| matches!(announcement, Announcement::Failed { .. })),
            "a command the user stopped did not fail"
        );
        assert_eq!(
            sink.rendered(),
            "partial output
",
            "what it managed to say still reaches the buffer"
        );
    }

    #[test]
    fn an_interrupt_clears_both_timers_like_any_other_ending() {
        let (mut actor, _clock, _sink) = actor();
        started(&mut actor);
        output(
            &mut actor, "text
",
        );

        actor.handle(SessionInput::CommandInterrupted {
            command_id: CommandId(1),
        });
        assert_eq!(
            actor.take_requests(),
            Requests {
                render: Wake::Clear,
                pacing: Wake::Clear,
            }
        );
    }

    #[test]
    fn alt_screen_transitions_are_idempotent() {
        let (mut actor, _clock, sink) = actor();
        actor.handle(SessionInput::AltScreenEntered);
        actor.handle(SessionInput::AltScreenEntered);
        actor.handle(SessionInput::AltScreenLeft);

        assert_eq!(
            sink.events(),
            vec![SessionEvent::AltScreenEntered, SessionEvent::AltScreenLeft],
            "a program redrawing does not re-enter"
        );
    }

    // --- The loop ---------------------------------------------------------------

    /// Yields until the recorder satisfies `done`, rather than guessing at a delay.
    /// Everything is in memory, so this converges in a turn or two; the bound turns a
    /// hang into a legible failure.
    async fn until(sink: &Recorder, what: &str, done: impl Fn(&[SessionEvent]) -> bool) {
        for _ in 0..1_000 {
            if done(&sink.events()) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("timed out waiting for {what}; saw {:?}", sink.events());
    }

    #[tokio::test]
    async fn the_loop_arms_a_timer_and_answers_it() {
        let config = PacingConfig::default();
        let clock = Arc::new(FakeClock::default());
        let sink = Arc::new(Recorder::default());
        let actor = SessionActor::new(config, clock.clone(), sink.clone());
        let (inputs, rx) = mpsc::unbounded_channel();
        let loop_done = tokio::spawn(actor.run(rx));

        inputs
            .send(SessionInput::CommandStarted {
                command_id: CommandId(1),
                command_line: None,
            })
            .expect("actor is running");
        inputs
            .send(SessionInput::Output {
                text: "hello\n".to_owned(),
            })
            .expect("actor is running");
        until(&sink, "the command to open", |events| !events.is_empty()).await;

        // Firing the deadline the policy asked for is the only thing that can produce
        // the announcement, so this passing means the loop really armed it.
        clock.advance_to(config.quiescence);
        until(&sink, "the chunk to be read", |events| {
            events.iter().any(|event| {
                matches!(
                    event,
                    SessionEvent::Announce {
                        announcement: Announcement::ReadAloud { .. },
                        ..
                    }
                )
            })
        })
        .await;

        drop(inputs);
        loop_done
            .await
            .expect("the loop ends when its inputs close");
    }
}
