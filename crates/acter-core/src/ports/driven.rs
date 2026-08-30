//! Facade over the driven ports — what the domain needs from the world, one file
//! per port. Adapters at the edges implement these.

mod clock;
mod connect_questions;
mod connect_sink;
mod event_sink;
mod explained;
mod installed_shells;
mod session_factory;
mod shell_adapter;
mod signatures;
mod ssh_questions;
mod terminal_engine;
mod transport;

pub use clock::{Clock, Timer};
pub use connect_questions::{
    ConnectQuestions, IF_YOU_SKIP, ProgramAnswer, ProgramQuestion, SetupAnswer, SetupQuestion,
};
pub use connect_sink::ConnectSink;
pub use event_sink::EventSink;
pub use explained::{Explained, NeverExplained};
pub use installed_shells::{InstalledShells, NoDistributions};
pub use session_factory::{Chosen, SessionFactory, Started};
pub use shell_adapter::{ShellAdapter, ShellFacts, ShellLaunch};
pub use signatures::{Signatures, Unchecked};
pub use ssh_questions::{
    HostKeyAnswer, HostKeyQuestion, HostKeyState, PasswordQuestion, Secret, SshQuestions, Unasked,
};
pub use terminal_engine::TerminalEngine;
pub use transport::{Transport, TransportError};
