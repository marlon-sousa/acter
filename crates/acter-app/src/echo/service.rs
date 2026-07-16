//! Service: the echo harness use case — returns the submitted text unchanged.

use super::api::EchoApi;

pub(crate) struct EchoService;

impl EchoApi for EchoService {
    fn echo(&self, text: &str) -> String {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Box<dyn EchoApi> {
        Box::new(EchoService)
    }

    #[test]
    fn echoes_text_unchanged() {
        assert_eq!(service().echo("git status"), "git status");
    }

    #[test]
    fn echoes_empty_and_unicode_text() {
        assert_eq!(service().echo(""), "");
        assert_eq!(service().echo("café ⚡"), "café ⚡");
    }
}
