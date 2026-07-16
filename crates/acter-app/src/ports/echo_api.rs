//! Port (driving): what the frontend may ask of the echo harness (temporary A1
//! domain; replaced by the session domain from A3 onward).

pub(crate) trait EchoApi: Send + Sync {
    /// Returns the text to display and announce for the given input.
    fn echo(&self, text: &str) -> String;
}
