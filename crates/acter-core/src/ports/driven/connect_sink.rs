//! Port (driven): where the steps of a connection go while it is being made.

use crate::ConnectStep;

pub trait ConnectSink: Send + Sync {
    fn send(&self, step: ConnectStep);
}
