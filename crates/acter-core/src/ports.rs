//! Facade over the crate's ports — every trait seam in the system, grouped by
//! direction. Driven ports (`driven/`) are what the domain needs from the world;
//! driving ports (`driving/`) are what the world may ask of the domain. Ports are
//! trait declarations only — no behavior, so no tests (the facade/router exemption).

mod driven;
mod driving;

pub use driven::{
    Clock, EventSink, HostKeyAnswer, HostKeyQuestion, HostKeyState, InstalledShells,
    NoDistributions, PasswordQuestion, Secret, SessionFactory, ShellAdapter, ShellFacts,
    ShellLaunch, SshQuestions, TerminalEngine, Timer, Transport, TransportError,
};
pub use driving::{ConnectApi, SessionApi};
