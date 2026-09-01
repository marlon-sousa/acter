//! Policy: what to ask a WSL distribution about its account's login shell, and what its
//! answer means.
//!
//! **`wsl.exe` is a client, not a shell** (spec B5.5). What it starts is whatever login
//! shell the distribution's account carries in its own passwd entry, and before this module
//! existed the crate assumed that was bash — [`injection`](crate::wsl::injection)'s
//! `PROMPT_COMMAND` program went into every distribution, including accounts running zsh,
//! fish or dash where nothing would ever read it.
//!
//! **The cheap half of B9's decision 7.** SSH asks the far end what it is on a channel of
//! its own; WSL needs neither a channel nor a protocol, only a second `wsl.exe`. What both
//! buy is the same: a sentence with a subject instead of a bare
//! `IntegrationUnavailable` five seconds later.
//!
//! **Asked on its own, never typed into the session.** A line typed into the session would
//! put a command nobody typed in front of a screen reader, which is B4.9's whole subject —
//! so this runs as its own invocation whose output the terminal buffer never sees.
//!
//! Pure: bytes in, a name out, no process. The invocation itself lives in
//! [`windows_machine`](crate::windows_machine), which is the one module in this crate that starts one,
//! and this is what lets the reading be tested against captured bytes rather than against
//! whatever distributions this particular computer happens to have (spec B5.3, decision 4,
//! applied again).

/// What is asked, in one line, so one invocation answers.
///
/// **`getent passwd` first, because that is where the answer actually lives.** The login
/// shell is the last field of the account's passwd entry, and `getent` reads it through the
/// name service switch — so it is right on a distribution whose accounts come from LDAP or
/// SSSD as well as on one whose accounts are in `/etc/passwd`.
///
/// **`$SHELL` second, because some distributions have no `getent`.** A busybox-only image
/// has none, and there `$SHELL` is the only thing left to ask. It is deliberately the
/// fallback and not the primary: `$SHELL` is what the *environment* says, which a user can
/// export to anything, where passwd is what `wsl.exe` will actually start.
///
/// **`$(id -un)` rather than `$USER`.** `wsl.exe -- <command>` runs without a login shell,
/// so the environment a login would have set up is not there and `$USER` is frequently
/// unset — measured as an empty argument, which makes `getent passwd` list *every* account
/// on the machine and the first line answer about root. `id -un` asks the kernel who this
/// is and needs nothing to have been set up.
///
/// `printf` rather than `echo` for the probe's reason: `echo`'s handling of backslashes and
/// flags differs between the very shells this is trying to tell apart.
pub(crate) const ASK: &str = r#"getent passwd "$(id -un)" 2>/dev/null || printf '%s\n' "$SHELL""#;

/// The shell named by what the distribution printed, or `None` when it named nothing this
/// program is willing to say out loud.
///
/// **UTF-8, unlike the distribution list.** `wsl.exe -l -q` writes UTF-16LE because that
/// output is `wsl.exe`'s own (see [`distributions`](super::distributions)); this output was
/// written by a program inside the distribution and passes through as the bytes it wrote.
/// Lossy rather than fallible for the same reason the list is: a shell name that will not
/// decode should cost the session nothing.
pub(crate) fn read(said: &[u8]) -> Option<String> {
    let said = String::from_utf8_lossy(said);
    let line = said.lines().map(str::trim).find(|line| !line.is_empty())?;
    basename(shell_field(line)?)
}

/// The last field of a passwd entry, or the whole line when the answer came from `$SHELL`.
///
/// A passwd entry has seven colon-separated fields and the shell is the last of them;
/// `$SHELL` is a bare path with no colons in it. Telling them apart by whether a colon is
/// present is enough, and it is enough *because* a path containing a colon is not a thing
/// on Linux the way it is on Windows.
fn shell_field(line: &str) -> Option<&str> {
    let field = match line.contains(':') {
        true => line.rsplit(':').next()?,
        false => line,
    };
    is_a_path(field).then_some(field)
}

/// Whether this looks like the path to a program rather than something else entirely.
///
/// **An answer that is not a path is itself a useful signal** (spec B9, decision 7, and the
/// same rule as the SSH probe's). An account with an empty shell field, a `$SHELL` that was
/// never expanded because no shell expanded it, and an error sentence that reached standard
/// output all land here — and all of them become "unrecognised" rather than being parsed
/// hopefully into a word Acter would then read aloud.
fn is_a_path(value: &str) -> bool {
    !value.is_empty() && !value.contains('$') && !value.contains(char::is_whitespace)
}

/// The program's own name, without the directories.
///
/// No leading `-` to strip, unlike the SSH probe's: that one reads `$0`-shaped answers from
/// a login shell, and a passwd entry stores a plain path.
fn basename(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    (!file.is_empty()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Ubuntu 24.04 under WSL actually printed for the developer's own account,
    /// measured 2026-08-29 — the case that must keep behaving exactly as it did before this
    /// entry existed.
    const UBUNTU: &[u8] = b"marlon:x:1000:1000:,,,:/home/marlon:/bin/bash\n";

    #[test]
    fn a_passwd_entry_names_the_shell_in_its_last_field() {
        assert_eq!(read(UBUNTU).as_deref(), Some("bash"));
    }

    /// **The case this whole entry exists for.** An account running zsh has always been
    /// started with bash's injection; now it is named instead.
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

    /// The busybox fallback: no `getent`, so `$SHELL` answered and it is a bare path with
    /// no colons to split on.
    #[test]
    fn a_bare_path_is_the_answer_when_getent_was_not_there() {
        assert_eq!(read(b"/bin/ash\n").as_deref(), Some("ash"));
    }

    /// An account whose passwd entry leaves the shell field empty runs whatever the system
    /// defaults to, which is not something this program knows — so it says nothing.
    #[test]
    fn an_account_with_no_shell_of_its_own_is_named_nothing() {
        assert_eq!(read(b"a:x:1000:1000::/home/a:\n"), None);
    }

    /// Nothing was expanded, because nothing ran: a distribution that would not start
    /// prints the variable back, or prints nothing at all.
    #[test]
    fn an_answer_that_is_not_a_path_names_nothing() {
        assert_eq!(read(b"$SHELL\n"), None, "no shell expanded it");
        assert_eq!(read(b""), None, "nothing answered");
        assert_eq!(read(b"\n  \n"), None, "blank lines are not an answer");
    }

    /// A sentence that reached standard output is prose, not a path, and is not read aloud
    /// as though it were a shell.
    #[test]
    fn a_complaint_on_standard_output_is_not_mistaken_for_a_shell() {
        let said = b"There is no distribution with the supplied name.\n";

        assert_eq!(read(said), None);
    }

    /// Only the first line that says anything is read. `getent` answers with one entry, but
    /// a distribution that prints a banner first must not make the answer unreadable.
    #[test]
    fn the_first_line_with_anything_in_it_is_the_answer() {
        let said = b"\n\na:x:1000:1000::/home/a:/bin/bash\nb:x:1001:1001::/home/b:/bin/zsh\n";

        assert_eq!(read(said).as_deref(), Some("bash"));
    }

    /// CRLF reaches this from a distribution whose output crossed the client, and a name
    /// with a carriage return on the end matches no shell and reads as an unexplained
    /// pause.
    #[test]
    fn a_carriage_return_is_not_part_of_the_name() {
        assert_eq!(
            read(b"a:x:1000:1000::/home/a:/bin/bash\r\n").as_deref(),
            Some("bash")
        );
    }

    /// The question asks passwd first and falls back rather than asking twice: one
    /// invocation, because this one sits in the seconds before there is a prompt.
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
