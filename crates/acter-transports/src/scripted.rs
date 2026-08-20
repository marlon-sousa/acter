//! Adapter: `ScriptedTransport` — the fake *pipe*. A [`Transport`] that carries a
//! [`FakeShell`]'s bytes instead of a process's.
//!
//! **It decides how bytes arrive, never what they say** (spec B3.6, decision 1). The
//! prompt, the echo, the line discipline, rule matching and the interrupt predicate all
//! live behind [`FakeShell`]; what is left here is the half that is genuinely about a
//! pipe: the task, the clock, the read channel, where each read ends, what was written,
//! the last resize, and the far end going away. `ScriptedTransport::new` composes the
//! ordinary case — a transcript, read whole — and
//! [`with_shell`](ScriptedTransport::with_shell) composes any other.
//!
//! **Every delay comes from the [`Clock`] port; nothing sleeps.** With B1.5's fake clock
//! a script's timing is honored exactly at zero real time, which is what makes the pacing
//! policy assertable; with `SystemClock` the same code is a manual session paced for a
//! human ear. Reaching for `tokio::time` here would force every timing test to either
//! sleep for real — B1.5 already recorded that Windows timer granularity makes short
//! waits unassertable — or drop the timing, and for `tail`, `burst` and `forever` the
//! timing *is* the thing under test (spec B3.5, decision 3).
//!
//! **An interrupt stays answerable during a wait, not merely between deliveries.** That
//! is the trap A3.1 hit once and fixed: an interrupt noticed only between deliveries
//! arrives one delivery late, which for a `forever` scenario means never. So the wait
//! itself watches the inbox, and asks the shell whether what arrived interrupts.
//!
//! **What it does not decide.** Which bytes the frontend sends on Ctrl+C is A3.2's
//! question. This transport answers whatever arrives: the built-in transcript marks both
//! the literal `stop` line and a written `0x03` as interrupting, so either answer already
//! has the other end of the wire (spec B3.5, decision 7).

pub(crate) mod transcript;

use std::collections::VecDeque;
use std::sync::Arc;

use acter_core::{Clock, Transport, TransportError};
use tokio::select;
use tokio::spawn;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::fake::{Chunking, FakeShell, Script, Submission, TranscriptShell};
use transcript::{DelayRange, Repeat};

pub use transcript::SessionTranscript;

/// The byte a terminal's line discipline carries an interrupt on, and the whole of what
/// this pipe knows about interrupting: it hands the far end the byte a written Ctrl+C
/// already delivers today, and the far end decides what that means.
///
/// Which submissions interrupt is the shell's knowledge and stays there — the built-in
/// transcript marks both the literal `stop` line and a written `0x03` as interrupting, so
/// this reaches the same rule a real Ctrl+C always did (spec B3.6 keeps that split, and
/// spec B6 decision 5 keeps this side of it one line).
const INTERRUPT: u8 = 0x03;

/// The starting state of the delay sampler. Any nonzero value will do; it is fixed so a
/// script with sampled ranges replays identically every run.
const ROLL_SEED: u64 = 0x_5EED_AC7E_5EED_AC7E;

/// One scripted session.
///
/// Constructed with the far end it carries and the clock it waits on, and driven
/// entirely through the [`Transport`] port. Nothing about it is test-only: DESIGN's
/// Decided item makes the scripted session a permanent supported session kind, and from
/// B6 onward it is the transport a profile selects rather than a service that imitates
/// one.
pub struct ScriptedTransport {
    /// The far end. Moved into the emission loop by [`Transport::start`], because a
    /// shell is a state machine and cannot be in two places at once.
    shell: Option<Box<dyn FakeShell>>,
    chunking: Chunking,
    clock: Arc<dyn Clock>,
    /// The emission task's inbox, carrying writes exactly as they arrived. `None` until
    /// [`Transport::start`], which is the one thing that distinguishes "not started"
    /// from "ended" for a caller.
    writes: Option<UnboundedSender<Vec<u8>>>,
    /// A handle on the read channel, kept only to notice that the far side let go.
    reads: Option<Sender<Vec<u8>>>,
    /// Every byte ever written, in order — including the device-query answers the
    /// terminal engine produced, which is what makes `TerminalEngine::take_replies`
    /// assertable end to end.
    written: Vec<u8>,
    last_resize: Option<(u16, u16)>,
}

impl ScriptedTransport {
    /// The ordinary composition: a transcript-backed shell, one delivery per read.
    pub fn new(transcript: SessionTranscript, clock: Arc<dyn Clock>) -> Self {
        Self::with_shell(
            Box::new(TranscriptShell::new(transcript)),
            Chunking::Whole,
            clock,
        )
    }

    /// Any far end, cut any way: an [`Unmarked`](crate::Unmarked) shell for a session
    /// with no integration, [`Chunking::Bytes`] to make every marker and every escape
    /// sequence arrive a byte at a time.
    pub fn with_shell(
        shell: Box<dyn FakeShell>,
        chunking: Chunking,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            shell: Some(shell),
            chunking,
            clock,
            writes: None,
            reads: None,
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
    /// Spawns the emission loop, which greets with the shell's prompt.
    ///
    /// Must be called from within a tokio runtime, which is true of every session actor
    /// — the same requirement `SystemClock::timer` carries for the same reason.
    ///
    /// Starting twice is not a restart: the far end moved into the first loop, so the
    /// second call has nothing to run. Its channel closes immediately and its writes
    /// report a session that has ended, which is the truthful answer available — a
    /// session is torn down and replaced, never restarted in place.
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let (writes, inbox) = unbounded_channel();
        self.writes = Some(writes);
        self.reads = Some(bytes.clone());
        let Some(shell) = self.shell.take() else {
            return;
        };
        let emitter = Emitter {
            shell,
            chunking: self.chunking,
            clock: Arc::clone(&self.clock),
            bytes,
            inbox,
            pending: Vec::new(),
            queued: VecDeque::new(),
            roll: ROLL_SEED,
        };
        spawn(emitter.run());
    }

    /// Records the bytes and hands them to the far end, unchanged and uncut.
    ///
    /// Which of them add up to a submitted command is the shell's to say, not the pipe's:
    /// the line discipline runs in the emission loop, where the shell lives, so a
    /// device-query answer written mid-line ends up recorded rather than mistaken for a
    /// command without this method having to know why.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let writes = self.writes.as_ref().ok_or(TransportError::NotStarted)?;
        if self.reads.as_ref().is_some_and(Sender::is_closed) {
            return Err(TransportError::Closed);
        }

        self.written.extend_from_slice(bytes);
        writes
            .send(bytes.to_vec())
            .map_err(|_| TransportError::Closed)
    }

    /// Delivered as the byte the far end already recognizes, through the same inbox a
    /// write uses — so it arrives during a wait rather than between two deliveries, which
    /// is what makes interrupting an endless sequence possible at all.
    ///
    /// Recorded in [`written`](Self::written) like any other byte: a scripted session
    /// keeps everything it was told, and an interrupt is something it was told.
    fn interrupt(&mut self) -> Result<(), TransportError> {
        self.write(&[INTERRUPT])
    }

    /// Accepted and recorded. A scripted session has no grid of its own to reflow — the
    /// terminal engine above it does, and it is resized separately — so the recorded
    /// value is the whole observable effect, and what a test asserts against.
    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        self.last_resize = Some((columns, screen_lines));
        Ok(())
    }
}

/// The emission loop: one task, owning the far end, the read channel and the write
/// inbox.
struct Emitter {
    shell: Box<dyn FakeShell>,
    chunking: Chunking,
    clock: Arc<dyn Clock>,
    bytes: Sender<Vec<u8>>,
    inbox: UnboundedReceiver<Vec<u8>>,
    /// Bytes written but not yet recognized as a submission. The buffer the shell's line
    /// discipline drains.
    pending: Vec<u8>,
    /// Submissions that arrived while a script was playing and did not interrupt it — a
    /// real shell's typeahead. Taken in order once the script ends.
    queued: VecDeque<Submission>,
    roll: u64,
}

/// The far end let go: the session is over and the loop returns.
struct Gone;

/// How a script ended.
enum Ran {
    /// Every delivery played.
    Completed,
    /// An interrupting submission arrived and this script stops here; that submission is
    /// answered next.
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
        if self.greet().await.is_err() {
            return;
        }
        while let Some(submission) = self.next_submission().await {
            let mut current = submission;
            loop {
                let answer = self.shell.answer(&current);
                match self.play(&answer).await {
                    Ran::Gone => return,
                    Ran::Interrupted(next) => current = next,
                    Ran::Completed => {
                        if self.greet().await.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        }
    }

    /// The prompt sequence, at the start of the session and after every answer that ran
    /// to completion. Nothing interrupts a prompt — it is instantaneous — so a
    /// submission that arrives during one is simply answered next.
    async fn greet(&mut self) -> Result<(), Gone> {
        let prompt = self.shell.greet();
        match self.play(&prompt).await {
            Ran::Completed => Ok(()),
            Ran::Interrupted(submission) => {
                self.queued.push_front(submission);
                Ok(())
            }
            Ran::Gone => Err(Gone),
        }
    }

    /// Plays one script, watching for an interrupt throughout every wait.
    async fn play(&mut self, script: &Script) -> Ran {
        for delivery in script.deliveries() {
            let mut left = match delivery.repeat() {
                Repeat::Times(times) => Some(times),
                // Endless: nothing counts down, and only an interrupt or the reader
                // going away ends it.
                Repeat::Endless(_) => None,
            };
            while left.is_none_or(|times| times > 0) {
                match self.wait(delivery.delay()).await {
                    Waited::Elapsed => {}
                    Waited::Interrupted(submission) => return Ran::Interrupted(submission),
                    Waited::Gone => return Ran::Gone,
                }
                if self.send(delivery.bytes()).await.is_err() {
                    return Ran::Gone;
                }
                left = left.map(|times| times - 1);
            }
        }
        Ran::Completed
    }

    /// Waits out one delivery's delay, staying answerable while it waits.
    ///
    /// A zero delay asks the clock for nothing at all: a timer for zero time would make
    /// every instant delivery — the echo, a marker the shell draws without pausing —
    /// depend on a clock tick that no scenario asked for.
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
                Some(Some(written)) => {
                    if let Some(interrupt) = self.take(written) {
                        return Waited::Interrupted(interrupt);
                    }
                }
            }
        }
    }

    /// The next thing to answer: typeahead first, then whatever is written next.
    async fn next_submission(&mut self) -> Option<Submission> {
        loop {
            if let Some(queued) = self.queued.pop_front() {
                return Some(queued);
            }
            let written = self.inbox.recv().await?;
            // Between two answers there is nothing to interrupt, so a submission that
            // would have interrupted is simply the next one answered — and it goes to
            // the front, because it arrived before anything queued behind it.
            if let Some(interrupt) = self.take(written) {
                self.queued.push_front(interrupt);
            }
        }
    }

    /// Hands written bytes to the shell's line discipline and files what comes back: the
    /// first submission the shell calls interrupting is returned, everything else queues
    /// as typeahead, and what was not a whole submission stays pending.
    fn take(&mut self, written: Vec<u8>) -> Option<Submission> {
        self.pending.extend_from_slice(&written);
        let submissions = self.shell.accept(&mut self.pending);
        let mut interrupt = None;
        for submission in submissions {
            if interrupt.is_none() && self.shell.interrupts(&submission) {
                interrupt = Some(submission);
            } else {
                self.queued.push_back(submission);
            }
        }
        interrupt
    }

    /// One delivery, cut into reads. Where those cuts fall is this side's decision and
    /// nothing the shell said — which is what makes every fixture replayable a byte at a
    /// time (spec B3.6, decision 3).
    ///
    /// `&mut self` rather than `&self` only because a shared borrow held across an await
    /// would require the far end to be `Sync`, and a [`FakeShell`] is a state machine
    /// with one owner: `Send`, deliberately not `Sync`.
    async fn send(&mut self, bytes: &[u8]) -> Result<(), Gone> {
        for read in self.chunking.cut(bytes) {
            self.bytes.send(read.to_vec()).await.map_err(|_| Gone)?;
        }
        Ok(())
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
    use std::mem::take;
    use std::sync::Mutex;
    use std::time::Duration;

    use acter_core::Timer;
    use tokio::sync::mpsc::{Receiver, channel};
    use tokio::sync::oneshot;
    use tokio::task::yield_now;

    use crate::Unmarked;

    use super::*;

    /// One read's worth of buffer. Large enough that no test ever observes a full
    /// channel, so what a test sees is what the script said and nothing about
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
            Self::over(Box::new(TranscriptShell::new(transcript)), Chunking::Whole)
        }

        fn over(shell: Box<dyn FakeShell>, chunking: Chunking) -> Self {
            let clock = Arc::new(FakeClock::default());
            let mut transport = ScriptedTransport::with_shell(shell, chunking, clock.clone());
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
        /// everything emitted on the way. One deadline at a time because a delivery arms
        /// its successor only once it has fired: jumping straight to `at` would deliver
        /// one step of a repeating sequence and leave the rest in the future.
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
            "an instant delivery asks the clock for nothing at all"
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

    /// The trap A3.1 hit once and fixed: an interrupt noticed only *between* deliveries
    /// arrives one delivery late, which for an endless sequence means never.
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
            "the cancelled delivery must never arrive, however long anyone waits"
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

    /// The port's method, on the implementer that ships with it. `interrupt` exists
    /// because over SSH an interrupt is a channel request rather than bytes in the data
    /// stream, so the service cannot compute bytes and call `write` — and here it lands
    /// exactly where a written Ctrl+C always did, which is the point: this pipe still
    /// knows nothing about what an interrupt *is* beyond which byte carries one.
    #[tokio::test]
    async fn interrupting_reaches_the_same_rule_a_written_control_byte_does() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;
        session.write("forever\n");
        let _echo = session.reads().await;
        let _running = session.advance_to(3000).await;

        session.transport.interrupt().expect("the session is open");

        let answer = texts(&session.reads().await);
        assert!(answer.contains(&"^C\r\n".to_owned()), "got: {answer:?}");
        assert!(
            answer.contains(&"\x1b]133;D\x07".to_owned()),
            "an interrupted command ends with no exit code to report: {answer:?}"
        );
        assert_eq!(
            session.transport.written(),
            b"forever\n\x03",
            "and a scripted session keeps every byte it was told, this one included"
        );
    }

    /// Where a read ends is the pipe's decision and nothing the shell said: the same
    /// prompt the transcript draws in three deliveries arrives one byte at a time,
    /// markers cut in half included. B3.6 decision 3 — what used to be a hand-authored
    /// `split_marker.json` is now a property of the carrier.
    #[tokio::test]
    async fn the_pipe_cuts_a_delivery_and_the_shell_never_does() {
        let mut session = Session::over(Box::new(TranscriptShell::builtin()), Chunking::Bytes(1));

        let reads = session.reads().await;
        assert!(
            reads.iter().all(|read| read.len() == 1),
            "every read is one byte: {:?}",
            texts(&reads)
        );
        assert_eq!(
            texts(&reads).concat(),
            "\x1b]133;A\x07acter> \x1b]133;B\x07",
            "and not a byte of the prompt was lost or moved"
        );
    }

    /// The unintegrated far end DESIGN's reliability case 2 is about: the same shell,
    /// answering the same lines, with no markers on the wire at all.
    #[tokio::test]
    async fn an_unmarked_shell_still_prompts_and_answers() {
        let mut session = Session::over(
            Box::new(Unmarked::new(TranscriptShell::builtin())),
            Chunking::Whole,
        );

        assert_eq!(
            texts(&session.reads().await),
            ["acter> "],
            "the prompt text is drawn and the markers around it are not"
        );

        session.write("small\n");
        let answered = texts(&session.advance_to(100).await).concat();
        assert_eq!(answered, "small\r\nhello from acter\r\nacter> ");
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

    /// A session is torn down and replaced, never restarted in place: the far end went
    /// into the first loop, and the second channel says so rather than pretending.
    #[tokio::test]
    async fn starting_twice_ends_the_second_session_rather_than_forking_the_far_end() {
        let mut session = Session::start(SessionTranscript::builtin());
        let _prompt = session.reads().await;

        let (sender, mut second) = channel(READS);
        session.transport.start(sender);
        yield_now().await;

        assert_eq!(
            second.try_recv().ok(),
            None,
            "nothing is emitted to a channel with no far end behind it"
        );
        assert_eq!(
            session.transport.write(b"small\n"),
            Err(TransportError::Closed)
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
