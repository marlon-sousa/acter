//! Facade over the driven ports — what the domain needs from the world, one file
//! per port. Adapters at the edges implement these.

mod event_sink;

pub use event_sink::EventSink;
