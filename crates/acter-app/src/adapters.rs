//! Facade for this crate's adapters, one file per adapter.

mod channel_sink;
mod system_clock;

pub(crate) use channel_sink::ChannelSink;
pub use system_clock::SystemClock;
