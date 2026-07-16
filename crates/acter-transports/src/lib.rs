//! Adapter crate: byte transports carrying a session's I/O, behind acter-core's
//! `Transport` port. Local ConPTY first; SSH behind a feature flag later.
//!
//! Facade: this file only declares modules and re-exports the public API.
#![warn(unreachable_pub)]
