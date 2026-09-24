//! Acter domain crate: entities, policies, ports, services and the IPC protocol
//! types. Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod controllers;
mod entities;
mod policies;
mod ports;
mod services;

pub use controllers::{Requests, SessionActor, SessionInput, Wake};
pub use entities::{
    AcceptedHostKey, Announcement, AttemptId, Colour, CommandId, ConnectAnswer, ConnectQuestion,
    ConnectStep, Connectable, Connected, ConnectionKind, ConnectionState, ExitCode, FORMAT, Fault,
    HostKeyOrigin, Integration, Key, KeyAck, KeyPress, LaunchRequest, LineId, LineOwner,
    LineRevision, MenuAction, Mode, Osc133Marker, PacingConfig, PacingState, PathStanding,
    ProfileId, Provenance, SavedConnection, SavedConnections, SavedRow, SavedTarget, Screen,
    SessionEvent, SessionId, SessionIntent, SessionSetup, SessionState, SetUp, ShellInstall,
    ShellMarkers, Signer, StoredSettings, Style, StyleRun, SubmitAck, TerminalItem, Variant,
    Verdict, join_runs, no_such_connection, refused, same_name, slice_runs,
};
pub use policies::{
    Anchor, Binding, BoundaryEvent, BoundaryTracker, Caret, Connection, FarEndAnswer, Keystroke,
    MenuItem, Region, RowChange, Standard, SystemMenu, TextSize, binding_for, catalogue, ended,
    far_end_row, key_bytes, measure, offered, system_menu,
};
pub use ports::{
    Chosen, Clock, ConnectApi, ConnectQuestions, ConnectSink, ConnectionStore, Cursor, EventSink,
    Explained, HostKeyAnswer, HostKeyQuestion, HostKeyState, HostKeyStore, IF_YOU_SKIP, LoginShell,
    NeverExplained, NoDistributions, PasswordQuestion, ProgramAnswer, ProgramQuestion,
    RememberedConnections, RememberedHostKeys, Secret, SessionApi, SessionFactory, SetupAnswer,
    SetupQuestion, ShellAdapter, ShellFacts, ShellLaunch, Signatures, SshQuestions, Started,
    StoredConnections, TerminalEngine, TerminalModes, ThisComputer, Timer, Transport,
    TransportError, Unasked, Unchecked,
};
pub use services::{ConnectService, Conversation, SessionService};
