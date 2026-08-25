//! Adapter crate: per-shell knowledge (PowerShell, cmd, bash) behind acter-core's
//! `ShellAdapter` port — how a shell is started, its shell-integration injection, and how
//! far its own markers reach.
//!
//! Facade: this file only declares modules and re-exports the public API.
//!
//! One shell so far, and the null adapter beside it. `cmd` is here rather than in B5
//! because 22.5 needed it: cmd's markers are one environment variable, and the domain
//! change they require had to land in the same PR as the injection or a marked session
//! goes silent (spec B4.5). B5.1 turned the constants it left behind into the port
//! ARCHITECTURE had named all along.
#![warn(unreachable_pub)]

mod cmd;
mod plain;
mod selection;

pub use cmd::Cmd;
pub use plain::Plain;
pub use selection::adapter_for;
