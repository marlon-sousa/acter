//! Controller: the per-session loop; every decision it makes belongs to `policies::autoread`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::entities::{Integration, ReadMode, UnspokenText};
use crate::policies::{PacingAction, measure, on_command_end, on_output, on_wake};
use crate::{
    Announcement, Clock, CommandId, ConnectionState, EventSink, ExitCode, LineId, LineRevision,
    Mode, PacingConfig, PacingState, SessionEvent, SessionState, StyleRun, Timer, join_runs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInput {
    /// `command_line` is `None` when the shell did not say what it is running.
    CommandStarted {
        command_id: CommandId,
        command_line: Option<String>,
    },
    /// `prompt` is true for a row the shell drew as its prompt, which reaches the buffer as output
    /// only in a shell that reports no exit code.
    Output {
        line: LineId,
        revision: LineRevision,
        text: String,
        spoken: bool,
        prompt: bool,
        runs: Vec<StyleRun>,
    },
    FarEndLine {
        text: Option<String>,
        caret: u32,
        anchored: bool,
    },
    PromptDrawn {
        text: String,
    },
    Connection {
        state: ConnectionState,
    },
    CommandEnded {
        command_id: CommandId,
        exit_code: ExitCode,
    },
    CommandInterrupted {
        command_id: CommandId,
    },
    NothingRan {
        command_id: CommandId,
    },
    MarkersObserved,
    GracePeriodExpired,
    FollowMode(bool),
    SelfTalk(bool),
    AltScreenEntered,
    AltScreenLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wake {
    #[default]
    Unchanged,
    Clear,
    After(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Requests {
    pub render: Wake,
    pub pacing: Wake,
}

#[derive(Debug)]
struct ActiveCommand {
    id: CommandId,
    started_at: Duration,
    pacing: PacingState,
    unrendered: Vec<Rendered>,
    unspoken: UnspokenText,
    render_armed: bool,
    /// Only spoken text moves it, so a rewrite never puts a line ending into the spoken span.
    last_line: Option<LineId>,
}

#[derive(Debug)]
struct Rendered {
    line: LineId,
    revision: LineRevision,
    text: String,
    prompt: bool,
    runs: Vec<StyleRun>,
}

impl ActiveCommand {
    fn new(id: CommandId, started_at: Duration) -> Self {
        Self {
            id,
            started_at,
            pacing: PacingState::default(),
            unrendered: Vec::new(),
            unspoken: UnspokenText::default(),
            render_armed: false,
            last_line: None,
        }
    }

    fn render(
        &mut self,
        line: LineId,
        revision: LineRevision,
        text: &str,
        prompt: bool,
        runs: Vec<StyleRun>,
    ) {
        if revision == LineRevision::Appended
            && let Some(last) = self.unrendered.last_mut()
            && last.line == line
        {
            join_runs(&mut last.runs, &last.text, &runs);
            last.text.push_str(text);
            last.prompt = prompt;
            return;
        }
        if revision != LineRevision::Appended {
            self.unrendered.retain(|pending| pending.line != line);
        }
        self.unrendered.push(Rendered {
            line,
            revision,
            text: text.to_owned(),
            prompt,
            runs,
        });
    }
}

pub struct SessionActor {
    config: PacingConfig,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EventSink>,
    session: SessionState,
    follow_mode: bool,
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
            session: SessionState::new(Mode::NonInteractive),
            follow_mode: false,
            self_talk: false,
            active: None,
            requests: Requests::default(),
        }
    }

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

    pub fn take_requests(&mut self) -> Requests {
        std::mem::take(&mut self.requests)
    }

    pub fn handle(&mut self, input: SessionInput) {
        match input {
            SessionInput::CommandStarted {
                command_id,
                command_line,
            } => self.command_started(command_id, command_line),
            SessionInput::Output {
                line,
                revision,
                text,
                spoken,
                prompt,
                runs,
            } => self.output(line, revision, &text, spoken, prompt, runs),
            SessionInput::CommandEnded {
                command_id,
                exit_code,
            } => self.command_ended(command_id, exit_code),
            SessionInput::CommandInterrupted { command_id } => self.command_interrupted(command_id),
            SessionInput::NothingRan { command_id } => self.nothing_ran(command_id),
            SessionInput::Connection { state } => {
                self.sink.send(SessionEvent::ConnectionChanged { state })
            }
            SessionInput::PromptDrawn { text } => {
                self.sink.send(SessionEvent::PromptDrawn { text })
            }
            SessionInput::FarEndLine {
                text,
                caret,
                anchored,
            } => {
                self.flush_render();
                self.sink.send(SessionEvent::FarEndLine {
                    text,
                    caret,
                    anchored,
                });
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

    pub fn wake_render(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.render_armed = false;
        }
        self.flush_render();
    }

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

    fn command_started(&mut self, command_id: CommandId, command_line: Option<String>) {
        self.active = Some(ActiveCommand::new(command_id, self.clock.now()));
        self.sink.send(SessionEvent::CommandStarted {
            command_id,
            command_line,
        });
    }

    fn output(
        &mut self,
        line: LineId,
        revision: LineRevision,
        text: &str,
        spoken: bool,
        prompt: bool,
        runs: Vec<StyleRun>,
    ) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        // The tick is not re-armed per chunk, or continuous output would starve rendering.
        // An empty rewrite is a row the far end erased, so only an empty append is skipped.
        if revision != LineRevision::Appended || !text.is_empty() {
            active.render(line, revision, text, prompt, runs);
            if !active.render_armed {
                active.render_armed = true;
                self.requests.render = Wake::After(self.config.render_tick);
            }
        }

        if !spoken {
            return;
        }
        let separated = match active.last_line {
            Some(last) if last != line => format!("\n{text}"),
            _ => text.to_owned(),
        };
        active.last_line = Some(line);
        active.unspoken.push(&separated, &self.config);
        let at = self.clock.now().saturating_sub(active.started_at);
        let (pacing, outcome) = on_output(
            active.pacing,
            &self.config,
            measure(&separated),
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

    fn command_interrupted(&mut self, command_id: CommandId) {
        if !self.close(SessionEvent::CommandInterrupted { command_id }) {
            return;
        }
        self.retire();
    }

    fn nothing_ran(&mut self, command_id: CommandId) {
        if !self.close(SessionEvent::CommandFinished { command_id }) {
            return;
        }
        self.retire();
    }

    /// The last word is sent before `terminal`: the frontend's completion beep for a `TooBig`
    /// command fires on the ending and must already have seen the verdict.
    ///
    /// `false` when no command was running.
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

    fn retire(&mut self) {
        self.active = None;
        self.requests.render = Wake::Clear;
        self.requests.pacing = Wake::Clear;
    }

    /// Every branch that speaks flushes the rendering path first, so nothing is announced
    /// before the buffer has it.
    fn apply(&mut self, action: PacingAction) {
        match self.aside(action) {
            PacingAction::None => {}
            PacingAction::Flush(mode) => {
                self.flush_render();
                let unintegrated = self.session.integration == Integration::Unintegrated;
                let Some(active) = self.active.as_mut() else {
                    return;
                };
                // Read before `take`, which resets the accumulator.
                let last_line = active.unspoken.last_line().map(str::to_owned);
                let (text, size) = active.unspoken.take();
                match mode {
                    ReadMode::Quiet => {}
                    ReadMode::Auto => {
                        if let Some(text) = text {
                            self.announce(Announcement::ReadAloud { text });
                        }
                    }
                    // Only an unintegrated session reads its last row: an integrated or pending
                    // one says its prompt through `PromptDrawn`.
                    ReadMode::TooBig => {
                        self.announce(Announcement::TooBig {
                            lines: size.lines.try_into().unwrap_or(u32::MAX),
                        });
                        if unintegrated && let Some(text) = last_line {
                            self.announce(Announcement::ReadAloud { text });
                        }
                    }
                }
            }
            PacingAction::StillRunning => self.announce(Announcement::StillRunning),
            PacingAction::OutputContinues => self.announce(Announcement::OutputContinues),
        }
    }

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
        let lines = std::mem::take(&mut active.unrendered);
        let command_id = active.id;
        for line in lines {
            self.sink.send(SessionEvent::Output {
                command_id,
                line: line.line,
                revision: line.revision,
                text: line.text,
                prompt: line.prompt,
                runs: line.runs,
            });
        }
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
    use crate::{Colour, Style};

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
        line_of(actor, 1, LineRevision::Appended, text, true)
    }

    fn line_of(
        actor: &mut SessionActor,
        line: u64,
        revision: LineRevision,
        text: &str,
        spoken: bool,
    ) -> Requests {
        styled(actor, line, revision, text, spoken, vec![])
    }

    fn styled(
        actor: &mut SessionActor,
        line: u64,
        revision: LineRevision,
        text: &str,
        spoken: bool,
        runs: Vec<StyleRun>,
    ) -> Requests {
        actor.handle(SessionInput::Output {
            line: LineId(line),
            revision,
            text: text.to_owned(),
            spoken,
            prompt: false,
            runs,
        });
        actor.take_requests()
    }

    fn ended(actor: &mut SessionActor, command_id: u32, exit_code: i32) {
        actor.handle(SessionInput::CommandEnded {
            command_id: CommandId(command_id),
            exit_code: ExitCode(exit_code),
        });
    }

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

        clock.advance_to(Duration::from_millis(400));
        let requests = output(&mut actor, "");
        assert_eq!(requests.pacing, Wake::After(Duration::from_millis(100)));

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
                line: LineId(1),
                revision: LineRevision::Appended,
                text: "text\n".to_owned(),
                prompt: false,
                runs: vec![],
            }),
            "rendering carries no verdict: the verdict rides an Announce"
        );
    }

    #[test]
    fn coalesced_appends_shift_the_later_runs_past_the_earlier_text() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        let red = Style {
            fg: Some(Colour::Named { index: 1 }),
            ..Style::default()
        };
        let bold = Style {
            bold: true,
            ..Style::default()
        };

        styled(
            &mut actor,
            1,
            LineRevision::Appended,
            "é: ",
            true,
            vec![run(0, 1, bold)],
        );
        styled(
            &mut actor,
            1,
            LineRevision::Appended,
            "red",
            true,
            vec![run(0, 3, red)],
        );
        actor.wake_render();

        assert_eq!(
            sink.events().last(),
            Some(&SessionEvent::Output {
                command_id: CommandId(1),
                line: LineId(1),
                revision: LineRevision::Appended,
                text: "é: red".to_owned(),
                prompt: false,
                runs: vec![run(0, 1, bold), run(3, 3, red)],
            })
        );
    }

    #[test]
    fn a_rewrite_replaces_the_runs_of_what_it_replaces() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
        let red = Style {
            fg: Some(Colour::Named { index: 1 }),
            ..Style::default()
        };

        styled(
            &mut actor,
            1,
            LineRevision::Appended,
            "abc",
            true,
            vec![run(0, 3, red)],
        );
        styled(&mut actor, 1, LineRevision::Rewritten, "abc", false, vec![]);
        actor.wake_render();

        assert_eq!(
            sink.events().last(),
            Some(&SessionEvent::Output {
                command_id: CommandId(1),
                line: LineId(1),
                revision: LineRevision::Rewritten,
                text: "abc".to_owned(),
                prompt: false,
                runs: vec![],
            })
        );
    }

    fn run(start: u32, len: u32, style: Style) -> StyleRun {
        StyleRun { start, len, style }
    }

    #[test]
    fn a_tripped_guard_keeps_rendering_and_stops_announcing() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);

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
    fn a_flood_in_an_unintegrated_session_still_says_where_the_far_end_is() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        started(&mut actor);

        for _ in 0..500 {
            output(&mut actor, "y\n");
        }
        output(&mut actor, "marlon@ubuntu:~$ ");
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![
                Announcement::TooBig { lines: 501 },
                Announcement::ReadAloud {
                    text: "marlon@ubuntu:~$ ".to_owned()
                },
            ],
            "the count, and then the one row it swallowed"
        );
    }

    #[test]
    fn a_flood_in_an_integrated_session_reads_no_extra_line() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::MarkersObserved);
        started(&mut actor);

        for _ in 0..500 {
            output(&mut actor, "y\n");
        }
        output(&mut actor, "marlon@ubuntu:~$ ");
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![Announcement::TooBig { lines: 501 }],
            "the size verdict, and nothing else: {:?}",
            sink.announcements()
        );
    }

    #[test]
    fn a_flood_before_the_grace_period_has_answered_reads_no_extra_line() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        started(&mut actor);

        for _ in 0..500 {
            output(&mut actor, "y\n");
        }
        output(&mut actor, "marlon@ubuntu:~$ ");
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![Announcement::TooBig { lines: 501 }],
            "nothing is claimed about a session nobody has answered for yet: {:?}",
            sink.announcements()
        );
    }

    #[test]
    fn a_flood_acter_started_itself_says_nothing_at_all() {
        let config = PacingConfig::default();
        let (mut actor, clock, sink) = actor();
        actor.handle(SessionInput::GracePeriodExpired);
        actor.handle(SessionInput::SelfTalk(true));
        started(&mut actor);

        for _ in 0..500 {
            output(&mut actor, "y\n");
        }
        output(&mut actor, "marlon@ubuntu:~$ ");
        clock.advance_to(config.quiescence);
        actor.wake_pacing();

        assert_eq!(
            sink.announcements(),
            vec![],
            "nothing about a command the user did not run: {:?}",
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
    fn a_block_nothing_ran_in_finishes_and_says_nothing() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);

        actor.handle(SessionInput::NothingRan {
            command_id: CommandId(1),
        });

        assert_eq!(sink.announcements(), vec![]);
        assert!(
            sink.events().contains(&SessionEvent::CommandFinished {
                command_id: CommandId(1)
            }),
            "and the block closes rather than being left running: {:?}",
            sink.events()
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

    #[test]
    fn an_unintegrated_session_renders_a_flood_and_announces_its_size() {
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

    #[test]
    fn the_last_word_on_a_command_is_said_before_the_event_that_ends_it() {
        let (mut actor, _clock, sink) = actor();
        started(&mut actor);
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
                line: LineId(0),
                revision: LineRevision::Appended,
                text: "hello\n".to_owned(),
                spoken: true,
                prompt: false,
                runs: vec![],
            })
            .expect("actor is running");
        until(&sink, "the command to open", |events| !events.is_empty()).await;

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
