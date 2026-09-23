//! Port: a trait seam between the domain and the world. Facade over the crate's
//! ports, grouped by direction; declares modules and re-exports their public API.

mod driven;
mod driving;

pub use driven::{
    Chosen, Clock, ConnectQuestions, ConnectSink, ConnectionStore, Cursor, EventSink, Explained,
    HostKeyAnswer, HostKeyQuestion, HostKeyState, HostKeyStore, IF_YOU_SKIP, LoginShell,
    NeverExplained, NoDistributions, PasswordQuestion, ProgramAnswer, ProgramQuestion,
    RememberedConnections, RememberedHostKeys, Secret, SessionFactory, SetupAnswer, SetupQuestion,
    ShellAdapter, ShellFacts, ShellLaunch, Signatures, SshQuestions, Started, StoredConnections,
    TerminalEngine, TerminalModes, ThisComputer, Timer, Transport, TransportError, Unasked,
    Unchecked,
};
pub use driving::{ConnectApi, SessionApi};
