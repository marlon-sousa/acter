//! Port (driven): which shells this person has had Acter's setup command explained for and
//! has said not to be asked about again.

pub trait Explained: Send + Sync {
    /// A store that cannot be read answers `false`, so the person is asked again.
    fn already(&self, shell: &str) -> bool;

    /// A failure to write is silent; the only consequence is that the dialog appears again.
    fn remember(&self, shell: &str);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NeverExplained;

impl Explained for NeverExplained {
    fn already(&self, _shell: &str) -> bool {
        false
    }

    fn remember(&self, _shell: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_nowhere_to_keep_it_every_shell_is_asked_about_again() {
        let explained = NeverExplained;
        explained.remember("bash");

        assert!(!explained.already("bash"));
    }
}
