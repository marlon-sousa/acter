//! Controller: the delivery layer — framework-free orchestration that owns runtime
//! machinery (tasks, channels, timers) and translates between the world's shape and the
//! domain's. Facade over the per-concept controller files.

mod session_actor;

pub use session_actor::{Requests, SessionActor, SessionInput, Wake};
