//! Adapter crate: per-shell knowledge (PowerShell, cmd, bash) behind acter-core's
//! `ShellAdapter` port — how a shell is started, its shell-integration injection, and how
//! far its own markers reach.
//!
//! Facade: this file only declares modules and re-exports the public API.
//!
//! `cmd` is here rather than in B5 because 22.5 needed it: cmd's markers are one
//! environment variable, and the domain change they require had to land in the same PR as
//! the injection or a marked session goes silent (spec B4.5). B5.1 turned the constants it
//! left behind into the port ARCHITECTURE had named all along, and B5.2 and B5.3 put two
//! more shells behind the same seam.
//!
//! **Three shells now, and two of them mark all four boundaries** — PowerShell in either
//! edition, and bash inside a WSL distribution. cmd marks two, which is all its `PROMPT`
//! can carry, and `Plain` marks nothing it was told about.
//!
//! **One module here is not a shell.** `installed` answers what this particular computer
//! has, which is I/O rather than knowledge and therefore a port of its own: a WSL adapter
//! with no distribution to name is not a session anyone can start (spec B5.3, decision 4).
#![warn(unreachable_pub)]

mod cmd;
mod far_end;
mod installed;
mod plain;
mod powershell;
mod selection;
mod wsl;

pub use cmd::Cmd;
pub use far_end::over_ssh;
pub use installed::ThisMachine;
pub use plain::Plain;
pub use powershell::PowerShell;
pub use selection::adapter_for;
pub use wsl::Wsl;
