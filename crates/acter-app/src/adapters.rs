//! Facade for this crate's adapters, one file per adapter.

mod channel_sink;
mod connect_steps;
mod explained_shells;
mod system_clock;
mod system_menu;

pub(crate) use channel_sink::ChannelSink;
pub(crate) use connect_steps::ConnectSteps;
pub(crate) use explained_shells::ExplainedShells;
pub use system_clock::SystemClock;
pub(crate) use system_menu::install as install_system_menu;
