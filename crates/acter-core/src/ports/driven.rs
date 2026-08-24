//! Facade over the driven ports — what the domain needs from the world, one file
//! per port. Adapters at the edges implement these.

mod clock;
mod event_sink;
mod shell_adapter;
mod terminal_engine;
mod transport;

pub use clock::{Clock, Timer};
pub use event_sink::EventSink;
pub use shell_adapter::{ShellAdapter, ShellLaunch};
pub use terminal_engine::TerminalEngine;
pub use transport::{Transport, TransportError};
