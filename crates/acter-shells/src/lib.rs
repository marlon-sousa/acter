//! Adapter crate: per-shell knowledge (PowerShell, cmd, bash) behind acter-core's
//! `ShellAdapter` port — shell-integration injection, quoting, completion strategy.
//!
//! Facade: this file only declares modules and re-exports the public API.
//!
//! One shell so far. `cmd` is here rather than in B5 because 22.5 needed it: cmd's markers
//! are one environment variable, and the domain change they require had to land in the
//! same PR as the injection or a marked session goes silent (spec B4.5).
#![warn(unreachable_pub)]

pub mod cmd;
