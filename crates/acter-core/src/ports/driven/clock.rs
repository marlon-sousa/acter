//! Port (driven): the passage of time — the one thing the session actor needs from the
//! world besides its event sink. Two capabilities, deliberately separate: reading how
//! much time has passed, and being woken later.
//!
//! `now` returns a [`Duration`] rather than an `Instant` so no handle on the real clock
//! ever crosses into the domain (B1 decision 3). It is measured from an origin the
//! implementation chooses and keeps to itself; the domain only ever subtracts two
//! readings, so the origin never matters.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::oneshot;

/// Reads monotonic elapsed time and hands out timers. Implemented by an adapter that
/// owns the real clock, and by a fake in tests that advances time by hand.
pub trait Clock: Send + Sync {
    /// Time since this clock's origin. Never decreases.
    fn now(&self) -> Duration;

    /// A timer that completes `after` has elapsed. Dropping it cancels the wait, so a
    /// re-armed deadline needs no explicit cancellation.
    fn timer(&self, after: Duration) -> Timer;
}

/// A pending wake-up. Awaited by the actor's loop; completed by whoever created it.
/// Wraps a channel rather than a boxed future so the trait stays dyn-compatible and a
/// fake clock can fire it on command with no runtime involved.
#[derive(Debug)]
pub struct Timer(oneshot::Receiver<()>);

impl Timer {
    /// Builds a timer from the receiving half of a channel the implementation fires.
    pub fn new(fired: oneshot::Receiver<()>) -> Self {
        Self(fired)
    }
}

impl Future for Timer {
    type Output = ();

    /// A dropped sender means the timer will never fire — the actor treats that the
    /// same as cancellation and simply never wakes on it, so the error is discarded.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map(|_| ())
    }
}
