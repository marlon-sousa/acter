//! Port (driving): what the frontend may ask of the echo harness.

pub(crate) trait EchoApi: Send + Sync {
    /// Returns the text to display and announce for the given input.
    fn echo(&self, text: &str) -> String;
}
