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

use crate::entities::UnspokenText;
use crate::policies::{PacingAction, measure, on_command_end, on_output, on_wake};
use crate::{
    Announcement, Clock, CommandId, EventSink, ExitCode, Mode, PacingConfig, PacingState, ReadMode,
    SessionEvent, SessionState, Timer,
};

/// A domain fact the actor is told about. Deliberately not bytes: extraction, OSC 133
/// recognition and PTY reading belong to B2/B3/B4, which later become the things that
/// feed this channel instead of a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInput {
    /// A command's output region has begun.
    CommandStarted {
        command_id: CommandId,
    },
    /// Text arrived for the running command. An empty chunk is not output and must not
    /// move the quiescence deadline (B1.1), which the policy enforces.
    Output {
        text: String,
    },
    /// The running command ended on its own.
    CommandEnded {
        command_id: CommandId,
        exit_code: ExitCode,
    },
    /// Follow mode was toggled. An explicit override: every chunk is read on arrival.
    FollowMode(bool),
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
            active: None,
            requests: Requests::default(),
        }
    }

    /// Runs until the input channel closes. Thin by design: every decision is in the
    /// synchronous methods below.
    pub async fn run(mut self, mut inputs: mpsc::Receiver<SessionInput>) {
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
            SessionInput::CommandStarted { command_id } => self.command_started(command_id),
            SessionInput::Output { text } => self.output(&text),
            SessionInput::CommandEnded {
                command_id,
                exit_code,
            } => self.command_ended(command_id, exit_code),
            SessionInput::FollowMode(on) => self.follow_mode = on,
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

    fn command_started(&mut self, command_id: CommandId) {
        self.active = Some(ActiveCommand::new(command_id, self.clock.now()));
        self.sink.send(SessionEvent::CommandStarted { command_id });
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
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let (pacing, outcome) = on_command_end(active.pacing, &self.config, active.unspoken.size());
        active.pacing = pacing;
        // Everything reaches the buffer before anything is said about it, including the
        // final remainder.
        self.flush_render();
        self.sink.send(SessionEvent::CommandFinished {
            command_id,
            exit_code,
            read_mode: ReadMode::Quiet,
        });
        self.apply(outcome.action);
        if exit_code.0 != 0 {
            self.announce(Announcement::Failed { exit_code });
        }
        self.active = None;
        self.requests.render = Wake::Clear;
        self.requests.pacing = Wake::Clear;
    }

    /// Turns one policy decision into events. Every branch that speaks flushes the
    /// rendering path first — the whole mechanism behind DESIGN's invariant.
    fn apply(&mut self, action: PacingAction) {
        match action {
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

    fn flush_render(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.unrendered.is_empty() {
            return;
        }
        let text = std::mem::take(&mut active.unrendered);
        let command_id = active.id;
        self.sink.send(SessionEvent::Output {
            command_id,
            text,
            // Render-only. The verdict rides an `Announce`; this field is vestigial and
            // is retired by A6.
            read_mode: ReadMode::Quiet,
        });
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
                read_mode: ReadMode::Quiet,
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
        let (inputs, rx) = mpsc::channel(8);
        let loop_done = tokio::spawn(actor.run(rx));

        inputs
            .send(SessionInput::CommandStarted {
                command_id: CommandId(1),
            })
            .await
            .expect("actor is running");
        inputs
            .send(SessionInput::Output {
                text: "hello\n".to_owned(),
            })
            .await
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
