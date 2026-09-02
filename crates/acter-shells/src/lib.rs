//! Adapter crate: per-shell knowledge (PowerShell, cmd, bash, zsh) behind acter-core's
//! `ShellAdapter` port — how a shell is started, what Acter runs inside it once it is up,
//! and how far its own markers reach.
//!
//! Facade: this file only declares modules and re-exports the public API.
//!
//! `cmd` is here rather than in B5 because 22.5 needed it: cmd's markers are one
//! environment variable, and the domain change they require had to land in the same PR as
//! the injection or a marked session goes silent (spec B4.5). B5.1 turned the constants it
//! left behind into the port ARCHITECTURE had named all along, and B5.2 and B5.3 put two
//! more shells behind the same seam.
//!
//! **Four shells now, and three of them mark all four boundaries** — PowerShell in either
//! edition, bash inside a WSL distribution, and since M2 whichever shell a Unix machine's
//! own account logs in to. cmd marks two, which is all its `PROMPT` can carry, and `Plain`
//! marks nothing it was told about.
//!
//! # Two questions here are about the machine rather than about a shell
//!
//! `windows_machine` and `unix_machine` answer what this particular computer has, which is
//! I/O rather than knowledge and therefore a port of its own: a WSL adapter with no
//! distribution to name is not a session anyone can start (spec B5.3, decision 4).
//! `windows_signatures` and `macos_signatures` answer who signed a file this machine would
//! start, which is I/O twice over — and which since B5.7 is the same question, because a
//! program is discovered as a *file* so that the file checked and the file started are one
//! thing (spec B5.7, decision 1).
//!
//! **Two adapters per port rather than one gated module, since M2.** ARCHITECTURE's
//! platform-divergence rule says a whole file that would be `#[cfg]`-gated has become an
//! adapter, and the composition root is where one of them is picked. `unix_machine` is
//! deliberately not `macos_machine`: `/etc/shells` and a passwd entry are POSIX, so Linux
//! will answer with this file rather than with a copy of it.
//!
//! **A signature adapter is gated whole and never stubbed**, which is the one place the rule
//! bends and it bends on purpose: each reads a system API that exists on one platform, and a
//! composition root that cannot build either uses acter-core's `Unchecked`, which vouches for
//! nothing. A stub that answered "trusted" because there was nothing to ask would be the
//! accept-everything mode this product does not have.
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
