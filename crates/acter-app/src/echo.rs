//! Facade for the echo harness modules (temporary A1 domain; replaced by the
//! session domain from A3 onward).

mod api;
mod service;

pub(crate) use api::EchoApi;
pub(crate) use service::EchoService;
