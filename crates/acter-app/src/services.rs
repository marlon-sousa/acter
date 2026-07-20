//! Facade for this crate's services, one file per service.

mod fake_session;

pub(crate) use fake_session::FakeSessionService;
