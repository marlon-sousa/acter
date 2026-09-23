//! Port: [`FakeShell`] — the seam between what the far end says and how its bytes
//! arrive, plus the vocabulary that seam is stated in.

use std::borrow::Cow;

use crate::scripted::transcript::{DelayRange, Repeat};

/// No method may wait: every wait belongs to the pipe, which holds the clock.
pub trait FakeShell: Send {
    /// The prompt sequence, asked for at the start of the session and after every answer
    /// that ran to completion.
    fn greet(&mut self) -> Script;

    /// Drains every complete submission from `pending` and leaves the remainder.
    fn accept(&mut self, pending: &mut Vec<u8>) -> Vec<Submission>;

    fn interrupts(&self, submission: &Submission) -> bool;

    /// What the far end says in answer, echo included.
    fn answer(&mut self, submission: &Submission) -> Script;
}

pub struct Script {
    deliveries: Vec<Delivery>,
}

pub(crate) struct Delivery {
    delay: DelayRange,
    bytes: Vec<u8>,
    repeat: Repeat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    bytes: Vec<u8>,
    terminated: bool,
}

impl Script {
    pub(crate) fn new(deliveries: Vec<Delivery>) -> Self {
        Self { deliveries }
    }

    pub(crate) fn deliveries(&self) -> &[Delivery] {
        &self.deliveries
    }

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

    pub(crate) fn line(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}
