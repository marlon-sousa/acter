//! Port (driven): the passage of time.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::oneshot;

pub trait Clock: Send + Sync {
    /// Time since an origin the implementation chooses; never decreases.
    fn now(&self) -> Duration;

    /// Dropping the returned timer cancels the wait.
    fn timer(&self, after: Duration) -> Timer;
}

#[derive(Debug)]
pub struct Timer(oneshot::Receiver<()>);

impl Timer {
    pub fn new(fired: oneshot::Receiver<()>) -> Self {
        Self(fired)
    }
}

impl Future for Timer {
    type Output = ();

    /// A dropped sender completes the timer, the same as firing it.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map(|_| ())
    }
}
