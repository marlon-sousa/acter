//! Facade for this crate's services, one file per service.

mod echo;

pub(crate) use echo::EchoService;
