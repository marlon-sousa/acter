//! Port: [`FakeShell`] — the seam between what the far end *says* and how its bytes
//! *arrive*, plus the vocabulary that seam is stated in.
//!
//! **Not one of acter-core's ports.** The domain never calls this and never sees it:
//! `ScriptedTransport` is the [`Transport`](acter_core::Transport) implementer, and this
//! trait is the cut *inside* that adapter. It is emphatically not `ShellAdapter` either,
//! which is a driven port the domain asks for shell *knowledge* and which sits outside
//! the byte path entirely (spec B3.6, decision 7).
//!
//! **What falls on which side.** To the shell: the prompt sequence, the echo, the line
//! discipline that decides when written bytes *are* a submission, which rule answers a
//! line, whether a submission interrupts what is in flight, markers, exit codes, and the
//! timing between deliveries — a command that dribbles output for a quarter of a second
//! is the program being slow, which is far-end behavior. To the pipe: how a produced
//! byte stream is cut into reads, what was written, the last resize, and the far end
//! going away (decision 1).
//!
//! **Synchronous and runtime-free, deliberately.** No clock, no channel, no `async`
//! anywhere below this line. That is this repo's established idiom for decision logic —
//! B1.5 made the actor's decisions synchronous methods precisely so its contract could
//! be asserted without a runtime, a sleep or a real clock — and it means an implementer
//! cannot accidentally acquire timing behavior of its own: every wait belongs to the
//! pipe, which holds the [`Clock`](acter_core::Clock), so "nothing anywhere sleeps"
//! stays a property of one file (decision 2).

use std::borrow::Cow;

use crate::scripted::transcript::{DelayRange, Repeat};

/// The far end of a scripted session: what it says, and when it says it.
///
/// A state machine, driven by the pipe: greeted once, then asked to answer whatever the
/// line discipline recognized. Every method is synchronous and none of them may wait —
/// see the module doc for why that is load-bearing rather than stylistic.
pub trait FakeShell: Send {
    /// The prompt sequence: at the start of the session, and again after every answer
    /// that ran to completion. An answer that was interrupted does not get one — the
    /// interrupting submission's own answer ends with the next prompt instead.
    fn greet(&mut self) -> Script;

    /// Takes bytes the way a shell's line discipline does, cutting everything complete
    /// out of `pending` and leaving the remainder.
    ///
    /// `&mut Vec<u8>` because line discipline *consumes*: what it recognized is drained
    /// and what it did not is left for the next write, which is how a device-query
    /// answer written mid-line ends up recorded rather than mistaken for a command.
    fn accept(&mut self, pending: &mut Vec<u8>) -> Vec<Submission>;

    /// Whether this submission cancels whatever is in flight rather than queueing behind
    /// it. Asked by the pipe *during* a wait, which is the trap A3.1 hit once: an
    /// interrupt noticed only between deliveries arrives one delivery late, and for an
    /// endless sequence that means never.
    fn interrupts(&self, submission: &Submission) -> bool;

    /// What the far end says in answer, echo included. The echo is the shell's because
    /// echoing is line-discipline behavior — it is the text B2 labels `CommandLine` and
    /// DESIGN's echo exclusion drops.
    fn answer(&mut self, submission: &Submission) -> Script;
}

/// The timing-bearing byte stream a shell wants produced — **with no read boundaries in
/// it**.
///
/// That absence is the point of the split. A transcript can no longer say where a read
/// ends, because where a read ends is a property of the pipe carrying the bytes rather
/// than of the far end that spoke them; the pipe's [`Chunking`](super::Chunking) decides
/// it, and every fixture is therefore replayable under every cut (decision 3).
pub struct Script {
    deliveries: Vec<Delivery>,
}

/// One delivery: a wait, then a payload, optionally repeated.
///
/// The bytes are already expanded — a shell hands the pipe bytes, never a format the
/// pipe would have to understand.
pub(crate) struct Delivery {
    delay: DelayRange,
    bytes: Vec<u8>,
    repeat: Repeat,
}

/// One thing the far end was told, on its way to being answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    bytes: Vec<u8>,
    /// Whether a line ending was consumed with it. A submission that ended a line is
    /// echoed with one; an interrupt byte is echoed as itself.
    terminated: bool,
}

impl Script {
    pub(crate) fn new(deliveries: Vec<Delivery>) -> Self {
        Self { deliveries }
    }

    pub(crate) fn deliveries(&self) -> &[Delivery] {
        &self.deliveries
    }

    /// Replaces every delivery's bytes, keeping its timing. What a decorator that
    /// changes *what* the far end says — but not *when* — is built out of.
    pub(crate) fn rewrite(&mut self, mut rewrite: impl FnMut(&[u8]) -> Vec<u8>) {
        for delivery in &mut self.deliveries {
            delivery.bytes = rewrite(&delivery.bytes);
        }
    }
}

impl Delivery {
    pub(crate) fn new(delay: DelayRange, bytes: Vec<u8>, repeat: Repeat) -> Self {
        Self {
            delay,
            bytes,
            repeat,
        }
    }

    /// A delivery with no wait before it, delivered once.
    pub(crate) fn instant(bytes: Vec<u8>) -> Self {
        Self::new(DelayRange::fixed(0), bytes, Repeat::default())
    }

    pub(crate) fn delay(&self) -> DelayRange {
        self.delay
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn repeat(&self) -> Repeat {
        self.repeat
    }
}

impl Submission {
    pub(crate) fn new(bytes: Vec<u8>, terminated: bool) -> Self {
        Self { bytes, terminated }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn terminated(&self) -> bool {
        self.terminated
    }

    /// The submitted bytes as a line, for matching. Lossy because a rule is authored as
    /// text and bytes that are not text can never equal one.
    pub(crate) fn line(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}
