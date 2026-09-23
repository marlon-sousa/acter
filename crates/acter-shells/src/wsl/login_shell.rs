//! Policy: what to ask a WSL distribution about its account's login shell, and what its
//! answer means.

/// `wsl.exe -- <command>` starts no login shell, so `$USER` is often unset, and `getent passwd`
/// given an empty name lists every account.
pub(crate) const ASK: &str = r#"getent passwd "$(id -un)" 2>/dev/null || printf '%s\n' "$SHELL""#;

/// `None` means the answer was not a path, so there is no shell to name.
///
/// UTF-8, unlike `wsl.exe -l -q`: `wsl.exe` passes a child's output through as the bytes it
/// wrote.
pub(crate) fn read(said: &[u8]) -> Option<String> {
    let said = String::from_utf8_lossy(said);
    let line = said.lines().map(str::trim).find(|line| !line.is_empty())?;
    basename(shell_field(line)?)
}

fn shell_field(line: &str) -> Option<&str> {
    let field = match line.contains(':') {
        true => line.rsplit(':').next()?,
        false => line,
    };
    is_a_path(field).then_some(field)
}

fn is_a_path(value: &str) -> bool {
    !value.is_empty() && !value.contains('$') && !value.contains(char::is_whitespace)
}

fn basename(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    (!file.is_empty()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UBUNTU: &[u8] = b"marlon:x:1000:1000:,,,:/home/marlon:/bin/bash\n";

    #[test]
    fn a_passwd_entry_names_the_shell_in_its_last_field() {
        assert_eq!(read(UBUNTU).as_deref(), Some("bash"));
    }

    #[test]
    fn an_account_that_does_not_run_bash_is_named_as_what_it_runs() {
        for (entry, shell) in [
            (&b"a:x:1000:1000::/home/a:/usr/bin/zsh\n"[..], "zsh"),
            (&b"a:x:1000:1000::/home/a:/usr/bin/fish\n"[..], "fish"),
            (&b"a:x:1000:1000::/home/a:/bin/dash\n"[..], "dash"),
            (&b"a:x:1000:1000::/home/a:/bin/sh\n"[..], "sh"),
        ] {
            assert_eq!(read(entry).as_deref(), Some(shell));
        }
    }

    #[test]
    fn a_bare_path_is_the_answer_when_getent_was_not_there() {
        assert_eq!(read(b"/bin/ash\n").as_deref(), Some("ash"));
    }

    #[test]
    fn an_account_with_no_shell_of_its_own_is_named_nothing() {
        assert_eq!(read(b"a:x:1000:1000::/home/a:\n"), None);
    }

    #[test]
    fn an_answer_that_is_not_a_path_names_nothing() {
        assert_eq!(read(b"$SHELL\n"), None, "no shell expanded it");
        assert_eq!(read(b""), None, "nothing answered");
        assert_eq!(read(b"\n  \n"), None, "blank lines are not an answer");
    }

    #[test]
    fn a_complaint_on_standard_output_is_not_mistaken_for_a_shell() {
        let said = b"There is no distribution with the supplied name.\n";

        assert_eq!(read(said), None);
    }

    #[test]
    fn the_first_line_with_anything_in_it_is_the_answer() {
        let said = b"\n\na:x:1000:1000::/home/a:/bin/bash\nb:x:1001:1001::/home/b:/bin/zsh\n";

        assert_eq!(read(said).as_deref(), Some("bash"));
    }

    #[test]
    fn a_carriage_return_is_not_part_of_the_name() {
        assert_eq!(
            read(b"a:x:1000:1000::/home/a:/bin/bash\r\n").as_deref(),
            Some("bash")
        );
    }

    #[test]
    fn the_question_prefers_passwd_and_falls_back_in_the_same_line() {
        assert!(ASK.starts_with("getent passwd"));
        assert!(ASK.contains("||"), "the fallback is in the same invocation");
        assert!(ASK.contains("$SHELL"));
        assert!(
            !ASK.contains("$USER"),
            "`wsl.exe -- <command>` runs no login shell, so $USER is frequently unset"
        );
    }
}
