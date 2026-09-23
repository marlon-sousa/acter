//! Adapter: `cmd.exe` behind the `ShellAdapter` port — how the shell is started, the
//! prompt string that carries OSC 133 markers, and the declaration of how far those
//! markers reach.

use acter_core::{ShellAdapter, ShellLaunch, ShellMarkers};

pub struct Cmd {
    program: String,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

/// `cmd.exe`'s line editor discards the pending line on escape; without it a submitted line
/// after ConPTY's cursor-position answer runs as `'s not recognized as an internal or external
/// command,`.
const ESCAPE: u8 = 0x1b;

impl ShellAdapter for Cmd {
    fn launch(&self) -> ShellLaunch {
        ShellLaunch {
            program: self.program.clone(),
            args: ARGS.iter().map(|arg| (*arg).to_owned()).collect(),
            environment: ENVIRONMENT
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn markers(&self) -> ShellMarkers {
        MARKERS
    }

    fn discards_line(&self) -> Option<u8> {
        Some(ESCAPE)
    }

    /// None: what ends a `cmd.exe` session has not been measured.
    fn eof(&self) -> Option<Vec<u8>> {
        None
    }
}

/// `$P$G` is `cmd.exe`'s own default prompt.
pub(crate) const PROMPT: &str = r"$e]133;A$e\$P$G$e]133;B$e\";

pub(crate) const ENVIRONMENT: &[(&str, &str)] = &[("PROMPT", PROMPT)];

/// `/Q` keeps the pseudoconsole's echo of what was typed the only echo in the stream, which
/// echo-based command correlation assumes.
pub(crate) const ARGS: &[&str] = &["/Q", "/K"];

pub(crate) const MARKERS: ShellMarkers = ShellMarkers::PromptAndCommandLine;

pub(crate) fn is_cmd(program: &str) -> bool {
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    program.eq_ignore_ascii_case("cmd") || program.eq_ignore_ascii_case("cmd.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_carries_both_markers_around_cmds_own_default() {
        assert!(PROMPT.starts_with(r"$e]133;A$e\"));
        assert!(PROMPT.ends_with(r"$e]133;B$e\"));
        assert!(PROMPT.contains("$P$G"));
    }

    #[test]
    fn the_environment_sets_the_prompt_and_nothing_else() {
        assert_eq!(ENVIRONMENT, &[("PROMPT", PROMPT)]);
    }

    #[test]
    fn cmd_is_recognized_however_it_was_named() {
        for named in ["cmd", "cmd.exe", "CMD.EXE", r"C:\Windows\system32\cmd.exe"] {
            assert!(is_cmd(named), "{named} is cmd");
        }
    }

    #[test]
    fn the_launch_carries_the_program_it_was_named_by_with_the_arguments_and_the_injection() {
        let launch = Cmd::new(r"C:\Windows\system32\cmd.exe").launch();

        assert_eq!(launch.program, r"C:\Windows\system32\cmd.exe");
        assert_eq!(launch.args, ["/Q", "/K"]);
        assert_eq!(
            launch.environment,
            [("PROMPT".to_owned(), PROMPT.to_owned())]
        );
    }

    #[test]
    fn cmd_claims_its_prompt_and_command_line_and_no_more() {
        assert_eq!(
            Cmd::new("cmd.exe").markers(),
            ShellMarkers::PromptAndCommandLine
        );
    }

    #[test]
    fn nothing_else_is_claimed() {
        for named in [
            "powershell.exe",
            "pwsh",
            "bash",
            r"C:\bin\wsl.exe",
            "cmdlet.exe",
        ] {
            assert!(!is_cmd(named), "{named} is not cmd");
        }
    }
}
