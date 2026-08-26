//! Facade for this crate's services, one file per service.
//!
//! A service coordinates ports, entities and policies for one named use case, and owns
//! the wiring and the lifetime of what it coordinates (ARCHITECTURE's service-sprawl
//! guard). It names no adapter: everything below it is a trait, and which implementation
//! fills each slot is the composition root's to decide.

mod connect;
mod session;

pub use connect::ConnectService;
pub use session::SessionService;
