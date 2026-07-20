//! Facade for this crate's entity/value types, one file per concept.

mod fake_script;

pub(crate) use fake_script::{DelayRange, FakeScript, parse};
