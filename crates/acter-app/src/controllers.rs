//! Facade for this crate's controllers, one file per controller.
//!
//! A controller orchestrates without owning domain rules. There is one: `Connecting`,
//! which exists because starting an SSH far end has to wait for a person and a Tauri
//! command must not (see the module for the deadlock that forces it).

mod connecting;

pub(crate) use connecting::Connecting;
