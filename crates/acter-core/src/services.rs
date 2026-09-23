//! Service: a domain's actionable surface. Facade over the crate's services,
//! one file per service; declares modules and re-exports their public API.

mod connect;
mod conversation;
mod session;

pub use connect::ConnectService;
pub use conversation::Conversation;
pub use session::SessionService;
