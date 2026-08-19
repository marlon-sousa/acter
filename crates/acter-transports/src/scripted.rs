//! Adapter: `ScriptedTransport` — a [`Transport`] that behaves like a shell, playing a
//! [`SessionTranscript`] instead of talking to a process.
//!
//! **It is a request/response loop, not a linear tape** (spec B3.5, decision 4). On
//! [`start`](Transport::start) it emits the transcript's prompt sequence: prompt-start,
//! the prompt text, command-line-start. A written line is echoed back the way a real
//! terminal echoes what was typed, the matching rule's steps play, and the prompt
//! returns. That is what B2's regions actually require — `Prompt` is A..B, `CommandLine`
//! is the B..C echo, `Output` is C..D — and an event-level fake can produce neither a
//! prompt nor an echo, which is why DESIGN's echo exclusion had never been exercised
//! against something that echoes.
//!
//! **Every delay comes from the [`Clock`] port; nothing sleeps.** With B1.5's fake clock
//! a transcript's timing is honored exactly at zero real time, which is what makes the
//! pacing policy assertable; with `SystemClock` the same code is a manual session paced
//! for a human ear. Reaching for `tokio::time` here would force every timing test to
//! either sleep for real — B1.5 already recorded that Windows timer granularity makes
//! short waits unassertable — or drop the timing, and for `tail`, `burst` and `forever`
//! the timing *is* the thing under test (decision 3).
//!
//! **What it does not decide.** Which bytes the frontend sends on Ctrl+C is A3.2's
//! question. This transport answers whatever arrives: a rule marked `interrupts` cancels
//! the sequence in flight, and the built-in transcript marks both the literal `stop`
//! line and a written `0x03`, so either answer already has the other end of the wire
//! (decision 7).

mod transcript;

use std::borrow::Cow;
use std::collections::VecDeque;
use std::mem::take;
use std::sync::Arc;

use acter_core::{Clock, Transport, TransportError};
use tokio::select;
use tokio::spawn;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

pub use transcript::SessionTranscript;
use transcript::{DelayRange, Repeat, Step};

/// The starting state of the delay sampler. Any nonzero value will do; it is fixed so a
/// transcript with sampled ranges replays identically every run.
const ROLL_SEED: u64 = 0x_5EED_AC7E_5EED_AC7E;

/// The line ending the fake's echo appends, which is what a terminal shows when Enter is
/// pressed: the carriage return moves to column one, the line feed moves down.
const CRLF: &[u8] = b"\r\n";

/// One scripted session.
///
/// Constructed with the transcript it plays and the clock it waits on, and driven
/// entirely through the [`Transport`] port. Nothing about it is test-only: DESIGN's
/// Decided item makes the scripted session a permanent supported session kind, and from
/// B6 onward it is the transport a profile selects rather than a service that imitates
/// one.
pub struct ScriptedTransport {
    transcript: Arc<SessionTranscript>,
    clock: Arc<dyn Clock>,
    /// The emission task's inbox. `None` until [`Transport::start`], which is the one
    /// thing that distinguishes "not started" from "ended" for a caller.
    submissions: Option<UnboundedSender<Submission>>,
    /// A handle on the read channel, kept only to notice that the far side let go.
    reads: Option<Sender<Vec<u8>>>,
    /// Bytes written but not yet a whole submission.
    partial: Vec<u8>,
    /// Every byte ever written, in order — including the device-query answers the
    /// terminal engine produced, which is what makes `TerminalEngine::take_replies`
    /// assertable end to end.
    written: Vec<u8>,
    last_resize: Option<(u16, u16)>,
}

impl ScriptedTransport {
    pub fn new(transcript: SessionTranscript, clock: Arc<dyn Clock>) -> Self {
        Self {
            transcript: Arc::new(transcript),
            clock,
            submissions: None,
            reads: None,
            partial: Vec::new(),
            written: Vec::new(),
            last_resize: None,
        }
    }

    /// Everything written to this transport so far, in order. A scripted session has
    /// nowhere to put bytes, so it keeps them: this is how a test sees that a device
    /// query was answered, and how a manual session can be inspected afterwards.
    pub fn written(&self) -> &[u8] {
        &self.written
    }

    /// The dimensions of the last [`Transport::resize`], if any.
    pub fn last_resize(&self) -> Option<(u16, u16)> {
        self.last_resize
    }
}

impl Transport for ScriptedTransport {
    /// Spawns the emission loop and greets with the prompt.
    ///
    /// Must be called from within a tokio runtime, which is true of every session actor
    /// — the same requirement `SystemClock::timer` carries for the same reason. Starting
    /// twice replaces the first loop: its inbox is dropped, so it ends at its next wait.
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let (submissions, inbox) = unbounded_channel();
        self.submissions = Some(submissions);
        self.reads = Some(bytes.clone());
        let emitter = Emitter {
            transcript: Arc::clone(&self.transcript),
            clock: Arc::clone(&self.clock),
            bytes,
            inbox,
            queued: VecDeque::new(),
            roll: ROLL_SEED,
        };
        spawn(emitter.run());
    }

    /// Takes bytes the way a shell's line discipline does: they accumulate until a line
    /// is complete, and each complete line becomes one submission.
    ///
    /// One exception, and it is decision 7's whole point: bytes that exactly match a rule
    /// marked `interrupts` are submitted without waiting for a line ending, because a
    /// control byte never carries one. Everything else — a device-query answer, a partial
    /// line — simply accumulates, which is also how those answers end up recorded rather
    /// than mistaken for commands.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let submissions = self
            .submissions
            .as_ref()
            .ok_or(TransportError::NotStarted)?;
        if self.reads.as_ref().is_some_and(Sender::is_closed) {
            return Err(TransportError::Closed);
        }

        self.written.extend_from_slice(bytes);
        self.partial.extend_from_slice(bytes);
        for submission in submissions_in(&mut self.partial, &self.transcript) {
            submissions
                .send(submission)
                .map_err(|_| TransportError::Closed)?;
        }
        Ok(())
    }

    /// Accepted and recorded. A scripted session has no grid of its own to reflow — the
    /// terminal engine above it does, and it is resized separately — so the recorded
    /// value is the whole observable effect, and what a test asserts against.
    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        self.last_resize = Some((columns, screen_lines));
        Ok(())
    }
}

/// One thing the far end was told, on its way to the emission loop.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Submission {
    bytes: Vec<u8>,
    /// Whether a line ending was consumed with it. A submission that ended a line is
    /// echoed with one; an interrupt byte is echoed as itself.
    terminated: bool,
}

impl Submission {
    /// The submitted bytes as a line, for matching. Lossy because a rule is authored as
    /// text and bytes that are not text can never equal one.
    fn line(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

/// Cuts everything complete out of the pending write buffer, leaving the remainder.
fn submissions_in(partial: &mut Vec<u8>, transcript: &SessionTranscript) -> Vec<Submission> {
    let mut submissions = Vec::new();
    loop {
        if let Some(index) = partial
            .iter()
            .position(|byte| *byte == b'\r' || *byte == b'\n')
        {
            let line = partial[..index].to_vec();
            // A carriage return and line feed together end one line, not two.
            let pair =
                usize::from(partial[index] == b'\r' && partial.get(index + 1) == Some(&b'\n'));
            partial.drain(..index + 1 + pair);
            submissions.push(Submission {
                bytes: line,
                terminated: true,
            });
            continue;
        }
        if !partial.is_empty() && transcript.interrupts(&String::from_utf8_lossy(partial)) {
            submissions.push(Submission {
                bytes: take(partial),
                terminated: false,
            });
            continue;
        }
        return submissions;
    }
}

/// The emission loop: one task, owning the read channel and the submission inbox.
struct Emitter {
    transcript: Arc<SessionTranscript>,
    clock: Arc<dyn Clock>,
    bytes: Sender<Vec<u8>>,
    inbox: UnboundedReceiver<Submission>,
    /// Submissions that arrived while a sequence was running and did not interrupt it —
    /// a real shell's typeahead. Taken in order once the sequence ends.
    queued: VecDeque<Submission>,
    roll: u64,
}

/// The far end let go: the session is over and the loop returns.
struct Gone;

/// How a rule's steps ended.
enum Ran {
    /// Every step played.
    Completed,
    /// A rule marked `interrupts` arrived and this sequence stops here; the interrupting
    /// submission is answered next.
    Interrupted(Submission),
    /// The reader is gone.
    Gone,
}

/// How one scripted wait ended.
enum Waited {
    Elapsed,
    Interrupted(Submission),
    Gone,
}

impl Emitter {
    async fn run(mut self) {
        if self.emit_prompt().await.is_err() {
            return;
        }
        while let Some(submission) = self.next_submission().await {
            let mut current = submission;
            loop {
                if self.echo(&current).await.is_err() {
                    return;
                }
                // Cloned so the borrowed rule outlives every `&mut self` call below: the
                // transcript is immutable and shared, which is exactly what an `Arc` is
                // for.
                let transcript = Arc::clone(&self.transcript);
                let rule = transcript.rule_for(&current.line());
                match self.run_steps(rule.steps()).await {
                    Ran::Gone => return,
                    Ran::Interrupted(next) => current = next,
                    Ran::Completed => {
                        if self.emit_prompt().await.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        }
    }

    /// The prompt sequence, at the start of the session and after every completed rule.
    /// Nothing interrupts a prompt — it is instantaneous — so a submission that arrives
    /// during one is simply answered next.
    async fn emit_prompt(&mut self) -> Result<(), Gone> {
        let transcript = Arc::clone(&self.transcript);
        match self.run_steps(transcript.prompt()).await {
            Ran::Completed => Ok(()),
            Ran::Interrupted(submission) => {
                self.queued.push_front(submission);
                Ok(())
            }
            Ran::Gone => Err(Gone),
        }
    }

    /// Plays one rule's steps, watching for an interrupt throughout every wait.
    async fn run_steps(&mut self, steps: &[Step]) -> Ran {
        for step in steps {
            let mut left = match step.repeat() {
                Repeat::Times(times) => Some(times),
                // Endless: nothing counts down, and only an interrupt or the reader
                // going away ends it.
                Repeat::Endless(_) => None,
            };
            while left.is_none_or(|times| times > 0) {
                match self.wait(step.delay()).await {
                    Waited::Elapsed => {}
                    Waited::Interrupted(submission) => return Ran::Interrupted(submission),
                    Waited::Gone => return Ran::Gone,
                }
                let Ok(payload) = self.transcript.expand(step.payload()) else {
                    // Validation resolved every payload when the transcript was loaded,
                    // so this means a capture file was removed underneath a running
                    // session. Ending is the honest answer: the session closes and the
                    // domain reports a session that ended, rather than playing a
                    // transcript with a hole in it.
                    return Ran::Gone;
                };
                if self.send(payload).await.is_err() {
                    return Ran::Gone;
                }
                left = left.map(|times| times - 1);
            }
        }
        Ran::Completed
    }

    /// Waits out one step's delay, staying answerable while it waits.
    ///
    /// The trap A3.1 hit once and fixed: an interrupt that is only noticed *between*
    /// steps arrives one step late, which for a `forever` scenario means never. So the
    /// wait itself is what watches the inbox.
    ///
    /// A zero delay asks the clock for nothing at all. A step with no delay exists to cut
    /// a read, not to wait, and a timer for zero time would make every such step depend
    /// on a clock tick that no scenario asked for.
    async fn wait(&mut self, delay: DelayRange) -> Waited {
        if delay.is_instant() {
            return Waited::Elapsed;
        }
        let roll = self.next_roll();
        let mut timer = self.clock.timer(delay.pick(roll));
        loop {
            // The borrows end with the select, so the arms below are free to touch
            // `self` again; `recv` is cancel-safe, so the message a losing branch was
            // waiting for is still there next time round.
            let received = select! {
                () = &mut timer => None,
                message = self.inbox.recv() => Some(message),
            };
            match received {
                None => return Waited::Elapsed,
                Some(None) => return Waited::Gone,
                Some(Some(submission)) => {
                    if self.transcript.interrupts(&submission.line()) {
                        return Waited::Interrupted(submission);
                    }
                    self.queued.push_back(submission);
                }
            }
        }
    }

    /// The next thing to answer: typeahead first, then whatever arrives.
    async fn next_submission(&mut self) -> Option<Submission> {
        match self.queued.pop_front() {
            Some(queued) => Some(queued),
            None => self.inbox.recv().await,
        }
    }

    /// Echoes what was submitted, as the terminal does with what was typed. This is the
    /// text B2 labels `CommandLine` and DESIGN's echo exclusion drops.
    async fn echo(&self, submission: &Submission) -> Result<(), Gone> {
        let mut bytes = submission.bytes.clone();
        if submission.terminated {
            bytes.extend_from_slice(CRLF);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        self.send(bytes).await
    }

    /// One delivery: one read for whoever is on the other end.
    async fn send(&self, bytes: Vec<u8>) -> Result<(), Gone> {
        self.bytes.send(bytes).await.map_err(|_| Gone)
    }

    /// The next value for a sampled delay range. An xorshift over a fixed seed rather
    /// than anything time-based: a sampled range only exists to keep manual pacing from
    /// sounding metronomic, and seeding it from the real clock would put a reading of
    /// real time inside a component whose whole point is that it never takes one.
    fn next_roll(&mut self) -> u64 {
        let mut roll = self.roll;
        roll ^= roll << 13;
        roll ^= roll >> 7;
        roll ^= roll << 17;
        self.roll = roll;
        roll
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    use acter_core::Timer;
    use tokio::sync::mpsc::{Receiver, channel};
    use tokio::sync::oneshot;
    use tokio::task::yield_now;

    use super::*;

    /// One read's worth of buffer. Large enough that no test ever observes a full
    /// channel, so what a test sees is what the transcript said and nothing about
    /// back-pressure.
    const READS: usize = 256;

    /// Time moves only when a test says so, and an armed timer fires only when its
    /// deadline is reached — B1.5's fake clock, which is why nothing here sleeps and the
    /// whole file runs in milliseconds. It also records what was asked for, because "the
    /// delays requested are exactly the transcript's" is itself a thing to assert.
    #[derive(Default)]
    struct FakeClock {
        now: Mutex<Duration>,
        armed: Mutex<Vec<(Duration, oneshot::Sender<()>)>>,
        requested: Mutex<Vec<Duration>>,
    }

    impl FakeClock {
        fn set_now(&self, at: Duration) {
            *self.now.lock().expect("clock poisoned") = at;
        }

        /// Moves to `at` and fires every timer due by then.
        fn advance_to(&self, at: Duration) {
            self.set_now(at);
            let mut armed = self.armed.lock().expect("timers poisoned");
            let (due, pending) = take(&mut *armed)
                .into_iter()
                .partition::<Vec<_>, _>(|(deadline, _)| *deadline <= at);
            *armed = pending;
            for (_, fire) in due {
                let _ = fire.send(());
            }
        }

        /// The soonest armed deadline, so a test can walk time from one wake to the next
        /// instead of jumping past several at once.
        fn next_deadline(&self) -> Option<Duration> {
            self.armed
                .lock()
                .expect("timers poisoned")
                .iter()
                .map(|(deadline, _)| *deadline)
                .min()
        }

        fn requested(&self) -> Vec<Duration> {
            self.requested.lock().expect("requests poisoned").clone()
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.now.lock().expect("clock poisoned")
        }

        fn timer(&self, after: Duration) -> Timer {
            self.requested
                .lock()
                .expect("requests poisoned")
                .push(after);
            let (fire, fired) = oneshot::channel();
            let deadline = self.now() + after;
            self.armed
                .lock()
                .expect("timers poisoned")
                .push((deadline, fire));
            Timer::new(fired)
        }
    }

    /// A started transport plus the reader on the other end of it.
    struct Session {
        transport: ScriptedTransport,
        reads: Receiver<Vec<u8>>,
        clock: Arc<FakeClock>,
    }

    impl Session {
        fn start(transcript: SessionTranscript) -> Self {
            let clock = Arc::new(FakeClock::default());
            let mut transport = ScriptedTransport::new(transcript, clock.clone());
            let (sender, reads) = channel(READS);
            transport.start(sender);
            Self {
                transport,
                reads,
                clock,
            }
        }

        fn write(&mut self, text: &str) {
            self.transport
                .write(text.as_bytes())
                .expect("the session is open");
        }

        /// Everything the emission loop has produced, up to the point where it is parked
        /// on its next timer. Deterministic on the current-thread runtime a `tokio::test`
        /// builds: the loop is only ever ready because a fake timer fired or a write
        /// arrived, and both of those are this test's doing.
        async fn reads(&mut self) -> Vec<Vec<u8>> {
            let mut reads = Vec::new();
            let mut quiet = 0;
            while quiet < 2 {
                yield_now().await;
                let mut produced = false;
                while let Ok(chunk) = self.reads.try_recv() {
                    reads.push(chunk);
                    produced = true;
                }
                quiet = if produced { 0 } else { quiet + 1 };
            }
            reads
        }

        /// Walks time forward one armed deadline at a time up to `at`, collecting
        /// everything emitted on the way. One deadline at a time because a step arms its
        /// successor only once it has fired: jumping straight to `at` would deliver one
        /// step of a repeating sequence and leave the rest in the future.
        async fn advance_to(&mut self, at: u64) -> Vec<Vec<u8>> {
            let at = Duration::from_millis(at);
            // Whatever is already ready runs first, so the loop below sees the timer the
            // emission loop is actually parked on rather than an empty schedule.
            let mut reads = self.reads().await;
            while let Some(next) = self.clock.next_deadline().filter(|next| *next <= at) {
                self.clock.advance_to(next);
                reads.extend(self.reads().await);
            }
            self.clock.set_now(at);
            reads
        }
    }

    fn texts(reads: &[Vec<u8>]) -> Vec<String> {
        reads
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect()
    }

    fn transcript(json: &str) -> SessionTranscript {
        SessionTranscript::parse(json).expect("the test transcript parses")
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
    }

    /// A prompt, and one rule that answers `go` with `n` deliveries of "tick" every
    /// `every` milliseconds.
    fn ticking(repeat: &str, every: u64) -> SessionTranscript {
        transcript(&format!(
            r#"{{
              "on_start": [{{ "payload": {{ "text": "> " }} }}],
              "rules": [
                {{
                  "match": "go",
                  "steps": [
                    {{
                      "delay": {{ "min_ms": {every}, "max_ms": {every} }},
                      "payload": {{ "text": "tick" }},
                      "repeat": {repeat}
                    }}
                  ]
                }}
              ],
              "default": {{ "steps": [] }}
            }}"#
        ))
    }

    #[tokio::test]
    async fn starting_emits_the_prompt_as_the_shell_would() {
        let mut session = Session::start(SessionTranscript::builtin());

        assert_eq!(
            texts(&session.reads().await),
            ["\x1b]133;A\x07", "acter> ", "\x1b]133;B\x07"],
            "prompt start, the prompt itself, then command-line start"
        );
    }

    #[tokio::test]
    async fn a_written_line_is_echoed_then_answered_then_the_prompt_returns() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("small\n");
        assert_eq!(
            texts(&session.reads().await),
            ["small\r\n", "\x1b]133;C\x07"],
            "the terminal echoes what was typed, then the command starts running"
        );

        assert_eq!(
            texts(&session.advance_to(100).await),
            [
                "hello from acter\r\n",
                "\x1b]133;D;0\x07",
                "\x1b]133;A\x07",
                "acter> ",
                "\x1b]133;B\x07"
            ],
            "output, the command ends, and the next prompt is drawn"
        );
    }

    #[tokio::test]
    async fn an_unrecognized_line_takes_the_default_rule() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("nothing scripted this\n");

        assert_eq!(
            texts(&session.reads().await),
            [
                "nothing scripted this\r\n",
                "\x1b]133;C\x07",
                "\x1b]133;D;0\x07",
                "\x1b]133;A\x07",
                "acter> ",
                "\x1b]133;B\x07"
            ],
            "echoed, opened and closed: A1's manual loop, with real boundaries"
        );
    }

    #[tokio::test]
    async fn the_delays_requested_are_exactly_the_transcripts() {
        let mut session = Session::start(transcript(
            r#"{
              "on_start": [{ "payload": { "text": "> " } }],
              "rules": [
                {
                  "match": "go",
                  "steps": [
                    { "payload": { "text": "instant" } },
                    { "delay": { "min_ms": 10, "max_ms": 10 }, "payload": { "text": "a" } },
                    { "delay": { "min_ms": 250, "max_ms": 250 }, "payload": { "text": "b" } }
                  ]
                }
              ],
              "default": { "steps": [] }
            }"#,
        ));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _all = session.advance_to(1000).await;

        assert_eq!(
            session.clock.requested(),
            [Duration::from_millis(10), Duration::from_millis(250)],
            "a step with no delay asks the clock for nothing at all"
        );
    }

    #[tokio::test]
    async fn a_counted_repeat_delivers_exactly_that_many_times() {
        let mut session = Session::start(ticking("3", 10));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;

        assert_eq!(
            texts(&session.advance_to(1000).await),
            ["tick", "tick", "tick", "> "],
            "three deliveries, then the prompt returns"
        );
        assert_eq!(texts(&session.advance_to(5000).await), [] as [String; 0]);
    }

    #[tokio::test]
    async fn an_endless_repeat_keeps_going() {
        let mut session = Session::start(ticking(r#""forever""#, 10));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;

        assert_eq!(session.advance_to(100).await.len(), 10);
        assert_eq!(
            session.advance_to(200).await.len(),
            10,
            "and it is still going"
        );
    }

    /// The trap A3.1 hit once and fixed: an interrupt noticed only *between* steps
    /// arrives one step late, which for an endless sequence means never.
    #[tokio::test]
    async fn an_interrupting_rule_cancels_a_sequence_while_it_is_waiting() {
        let mut session = Session::start(transcript(
            r#"{
              "on_start": [{ "payload": { "text": "> " } }],
              "rules": [
                {
                  "match": "go",
                  "steps": [
                    {
                      "delay": { "min_ms": 1000, "max_ms": 1000 },
                      "payload": { "text": "far too late" }
                    }
                  ]
                },
                {
                  "match": "stop",
                  "interrupts": true,
                  "steps": [{ "payload": { "text": "^C\r\n" } }]
                }
              ],
              "default": { "steps": [] }
            }"#,
        ));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;
        assert!(session.advance_to(500).await.is_empty(), "still waiting");

        session.write("stop\n");
        assert_eq!(
            texts(&session.reads().await),
            ["stop\r\n", "^C\r\n", "> "],
            "the interrupt is answered where the wait was, and the prompt returns"
        );

        assert!(
            session.advance_to(60_000).await.is_empty(),
            "the cancelled step must never arrive, however long anyone waits"
        );
    }

    /// A control byte carries no line ending, so waiting for one would mean an interrupt
    /// that never lands. Whether the frontend sends this byte at all is A3.2's question;
    /// what this fixes is that both answers have somewhere to arrive.
    #[tokio::test]
    async fn an_interrupt_byte_needs_no_line_ending() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;
        session.write("forever\n");
        let _echo = session.reads().await;
        let _running = session.advance_to(3000).await;

        session.write("\u{3}");

        let answer = texts(&session.reads().await);
        assert!(answer.contains(&"^C\r\n".to_owned()), "got: {answer:?}");
        assert!(
            answer.contains(&"\x1b]133;D\x07".to_owned()),
            "an interrupted command ends with no exit code to report: {answer:?}"
        );
    }

    /// The whole reason the format has steps rather than one blob per rule: a delivery is
    /// a read, so a fixture can cut one wherever DESIGN's reliability model needs it.
    #[tokio::test]
    async fn a_marker_split_across_two_steps_arrives_as_two_reads() {
        let mut session = Session::start(
            SessionTranscript::load(fixture("split_marker.json")).expect("the fixture loads"),
        );
        let _prompt = session.reads().await;

        session.write("split\n");
        let _echo = session.reads().await;

        assert_eq!(
            texts(&session.advance_to(100).await)[..3],
            ["output line\r\n", "\x1b]133;", "D;0\x07"],
            "the marker is cut in half, and each half is its own read"
        );
    }

    #[tokio::test]
    async fn a_write_before_the_session_starts_is_a_speakable_error() {
        let mut transport =
            ScriptedTransport::new(SessionTranscript::builtin(), Arc::new(FakeClock::default()));

        assert_eq!(transport.write(b"small\n"), Err(TransportError::NotStarted));
    }

    #[tokio::test]
    async fn a_write_after_the_session_ends_is_a_speakable_error() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        // The reader going away is what ends a session: the port has no other ending.
        let Session {
            mut transport,
            reads,
            ..
        } = session;
        drop(reads);

        let error = transport
            .write(b"small\n")
            .expect_err("the session has ended");
        assert_eq!(error, TransportError::Closed);
        assert_eq!(
            error.to_string(),
            "The session has ended, so the text could not be sent."
        );
    }

    #[tokio::test]
    async fn a_resize_is_accepted_and_recorded() {
        let mut session = Session::start(SessionTranscript::builtin());

        assert_eq!(session.transport.last_resize(), None);
        session
            .transport
            .resize(100, 30)
            .expect("a resize is accepted");
        assert_eq!(session.transport.last_resize(), Some((100, 30)));
    }

    /// A device-query answer is written back mid-line and carries no line ending, so it
    /// must not be mistaken for a submitted command — and it must still be visible to
    /// whoever is checking that the answer was sent at all.
    #[tokio::test]
    async fn bytes_written_are_recorded_and_a_partial_line_submits_nothing() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        session.write("\x1b[1;1R");

        assert_eq!(session.transport.written(), b"\x1b[1;1R");
        assert!(
            session.reads().await.is_empty(),
            "an unterminated write is not a command"
        );
    }

    /// A shell takes what is typed while it is busy and answers it afterwards. The fake
    /// does the same, so a test that submits twice sees both answers rather than losing
    /// one.
    #[tokio::test]
    async fn a_line_written_while_a_sequence_runs_is_answered_after_it() {
        let mut session = Session::start(ticking("1", 100));
        let _prompt = session.reads().await;

        session.write("go\n");
        let _echo = session.reads().await;
        session.write("later\n");
        assert!(
            session.reads().await.is_empty(),
            "the first answer is still running"
        );

        assert_eq!(
            texts(&session.advance_to(100).await),
            ["tick", "> ", "later\r\n", "> "],
            "the running sequence finishes, then the typeahead is answered"
        );
    }
}
