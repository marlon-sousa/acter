//! Facade over the crate's ports — every trait seam in the system, grouped by
//! direction. Driven ports (`driven/`) are what the domain needs from the world;
//! driving ports (`driving/`) are what the world may ask of the domain. Ports are
//! trait declarations only — no behavior, so no tests (the facade/router exemption).

mod driven;
mod driving;

pub use driven::{
    Chosen, Clock, ConnectQuestions, ConnectSink, Cursor, EventSink, Explained, HostKeyAnswer,
    HostKeyQuestion, HostKeyState, IF_YOU_SKIP, LoginShell, NeverExplained, NoDistributions,
    PasswordQuestion, ProgramAnswer, ProgramQuestion, Secret, SessionFactory, SetupAnswer,
    SetupQuestion, ShellAdapter, ShellFacts, ShellLaunch, Signatures, SshQuestions, Started,
    TerminalEngine, TerminalModes, ThisComputer, Timer, Transport, TransportError, Unasked,
    Unchecked,
};
pub use driving::{ConnectApi, SessionApi};
