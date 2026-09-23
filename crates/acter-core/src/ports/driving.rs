//! Facade over the driving ports: what the world may ask of the domain.

mod connect_api;
mod session_api;

pub use connect_api::ConnectApi;
pub use session_api::SessionApi;
