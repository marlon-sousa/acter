//! Facade over the driving ports — what the world may ask of the domain, one file
//! per port. Services implement these; controllers/routers depend on them.

mod session_api;

pub use session_api::SessionApi;
