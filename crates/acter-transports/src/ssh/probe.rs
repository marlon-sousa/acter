//! Adapter: asking the far end what it is, on a channel of its own, before the session
//! exists.

use std::time::Duration;

use russh::{ChannelMsg, client};
use tokio::time::timeout;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FarEnd {
    pub shell: Option<String>,
    pub flavour: Option<String>,
}

impl FarEnd {
    pub fn unknown() -> Self {
        Self::default()
    }

    /// `None` when neither a version variable nor a path to a shell was given.
    pub fn name(&self) -> Option<String> {
        if let Some(flavour) = &self.flavour {
            return Some(flavour.clone());
        }
        self.shell.as_deref().and_then(basename)
    }
}

/// `$0` must stay out: it is a parse error in fish and takes the whole line with it.
const ASK: &str = "printf 'ACTER SHELL=[%s] BASH=[%s] ZSH=[%s] FISH=[%s]\\n' \
                   \"$SHELL\" \"$BASH_VERSION\" \"$ZSH_VERSION\" \"$FISH_VERSION\"";

pub(crate) const PATIENCE: Duration = Duration::from_secs(3);

pub(crate) async fn ask<H: client::Handler>(
    connection: &mut client::Handle<H>,
    patience: Duration,
) -> FarEnd {
    match timeout(patience, exec(connection)).await {
        Ok(Some(said)) => read(&said),
        Ok(None) | Err(_) => FarEnd::unknown(),
    }
}

async fn exec<H: client::Handler>(connection: &mut client::Handle<H>) -> Option<String> {
    let mut channel = connection.channel_open_session().await.ok()?;
    channel.exec(true, ASK).await.ok()?;

    let mut said = String::new();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => said.push_str(&String::from_utf8_lossy(&data)),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Some(said)
}

fn read(said: &str) -> FarEnd {
    FarEnd {
        shell: field(said, "SHELL").filter(|value| is_a_path(value)),
        flavour: flavour(said),
    }
}

fn flavour(said: &str) -> Option<String> {
    [("BASH", "bash"), ("ZSH", "zsh"), ("FISH", "fish")]
        .into_iter()
        .find(|(variable, _)| field(said, variable).is_some_and(|value| !value.is_empty()))
        .map(|(_, name)| name.to_owned())
}

fn field(said: &str, name: &str) -> Option<String> {
    let at = said.find(&format!("{name}=["))? + name.len() + 2;
    let rest = said.get(at..)?;
    let end = rest.find(']')?;
    Some(rest[..end].to_owned())
}

fn is_a_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('$')
        && !value.contains(char::is_whitespace)
        && basename(value).is_some()
}

fn basename(path: &str) -> Option<String> {
    let file = path.rsplit(['/', '\\']).next()?;
    let file = file.strip_prefix('-').unwrap_or(file);
    (!file.is_empty()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the rig's `acter` account printed, under bash 5.2.15.
    const BASH: &str = "ACTER SHELL=[/bin/bash] BASH=[5.2.15(1)-release] ZSH=[] FISH=[]\n";
    /// The rig's `dashuser` account, whose dash sets no version variable.
    const DASH: &str = "ACTER SHELL=[/bin/dash] BASH=[] ZSH=[] FISH=[]\n";

    #[test]
    fn a_shell_that_names_itself_is_taken_at_its_word() {
        let far_end = read(BASH);

        assert_eq!(far_end.flavour.as_deref(), Some("bash"));
        assert_eq!(far_end.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(far_end.name().as_deref(), Some("bash"));
    }

    #[test]
    fn a_shell_that_names_nothing_is_named_by_its_path() {
        let far_end = read(DASH);

        assert_eq!(far_end.flavour, None, "dash sets no version variable");
        assert_eq!(far_end.name().as_deref(), Some("dash"));
    }

    #[test]
    fn an_answer_that_is_not_a_path_names_nothing() {
        let far_end = read("ACTER SHELL=[$SHELL] BASH=[] ZSH=[] FISH=[]");

        assert_eq!(far_end.shell, None);
        assert_eq!(far_end.name(), None, "nothing is said rather than nonsense");
    }

    #[test]
    fn a_far_end_that_said_nothing_is_simply_unknown() {
        assert_eq!(read(""), FarEnd::unknown());
        assert_eq!(FarEnd::unknown().name(), None);
    }

    #[test]
    fn what_is_running_beats_what_was_configured() {
        let far_end = read("ACTER SHELL=[/bin/sh] BASH=[5.2.15(1)-release] ZSH=[] FISH=[]");

        assert_eq!(far_end.name().as_deref(), Some("bash"));
    }

    #[test]
    fn the_name_is_the_program_rather_than_the_path_to_it() {
        assert_eq!(basename("/usr/local/bin/fish").as_deref(), Some("fish"));
        assert_eq!(
            basename("-bash").as_deref(),
            Some("bash"),
            "a login shell's leading dash is not part of its name"
        );
        assert_eq!(basename(""), None);
    }
}
