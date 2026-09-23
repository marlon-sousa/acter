//! Adapter crate: per-shell knowledge (PowerShell, cmd, bash, zsh) behind acter-core's
//! `ShellAdapter` port.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]

mod cmd;
mod far_end;
#[cfg(target_os = "macos")]
mod macos_signatures;
mod plain;
mod powershell;
mod selection;
mod setup;
mod unix_machine;
mod unix_shell;
mod windows_machine;
#[cfg(windows)]
mod windows_signatures;
mod wsl;

pub use cmd::Cmd;
pub use far_end::over_ssh;
#[cfg(target_os = "macos")]
pub use macos_signatures::AppleTrust;
pub use plain::Plain;
pub use powershell::PowerShell;
pub use selection::adapter_for;
pub use setup::setup_for;
pub use unix_machine::UnixMachine;
pub use unix_shell::UnixShell;
pub use windows_machine::WindowsMachine;
#[cfg(windows)]
pub use windows_signatures::WindowsTrust;
#[cfg(windows)]
pub(crate) use windows_signatures::target as signature_target;
pub use wsl::{Wsl, is_wsl};
