//! Adapter: what Acter knows about a server's identity — which host keys have been seen
//! before, which one *changed*, and where a newly accepted one is written down.
//!
//! **Two files, read the same way and written very differently** (spec B9, decision 5).
//! Acter reads the user's own `~/.ssh/known_hosts` and **never writes it**: reading it
//! means a host they already trust connects without being asked again, which is what stops
//! a populated `known_hosts` turning into a sequence of prompts; not writing it means a bug
//! here breaks Acter and not their `ssh`. What Acter accepts goes in a file of its own, in
//! the same format, so it stays inspectable with the tools they already have.
//!
//! **Neither path is decided here.** Both are handed in, because resolving
//! `%APPDATA%\acter` or `ACTER_PROFILES_DIR` is reading the environment, and this project
//! reads the environment in exactly one place — the composition root (spec B8, decision 2).
//! The reward is that every case below is testable against a directory made for the test,
//! including the ones a developer's own machine could never be put into.
//!
//! **A file that cannot be read is not a failure to connect.** It contributes nothing and
//! says so, as an aside on the question the user is then asked. The alternative — refusing
//! to connect because one line of somebody's `known_hosts` is a format `ssh-key` does not
//! parse — would make Acter fail where `ssh` succeeds, over a file Acter does not even own.
//! What it costs is that a *changed* key recorded only in an unreadable file is asked about
//! as an unknown one, and that is why the aside exists rather than being left implied: the
//! user is told the comparison did not happen.

use std::path::{Path, PathBuf};

use acter_core::{HostKeyQuestion, HostKeyState, ended};
use russh::keys::known_hosts::{known_host_keys_path, learn_known_hosts_path};
use russh::keys::{Algorithm, HashAlg, PublicKey};

/// The two records of which servers are which.
pub struct KnownHosts {
    /// Acter's own file, read and appended to.
    ours: PathBuf,
    /// The user's own `known_hosts`, read and never written. `None` on a machine where
    /// there is no home directory to look in, which is not an error and not a question.
    theirs: Option<PathBuf>,
}

/// Which file a sentence is about, in words a listener can act on.
///
/// Neither says a path. A `known_hosts` path spoken aloud is a long string of directory
/// names in which the one useful word — *whose* file it is — arrives last if at all.
const OURS: &str = "Acter's own record of host keys";
const THEIRS: &str = "your own OpenSSH known hosts file";

impl KnownHosts {
    /// Both files, as the composition root resolved them.
    pub fn new(ours: PathBuf, theirs: Option<PathBuf>) -> Self {
        Self { ours, theirs }
    }

    /// What to ask about this server's key, or `None` when there is nothing to ask.
    ///
    /// **`None` is the whole point of reading the user's file**: a key either of the two
    /// records already holds is a key nobody is asked about again.
    ///
    /// A key recorded under a *different algorithm* counts as unknown rather than changed,
    /// which is what `ssh` does too: a server that has grown an ed25519 key beside its RSA
    /// one has not changed identity, and calling that "the host key has changed" would
    /// spend the one alarming sentence this product has on a routine event.
    pub fn check(&self, host: &str, port: u16, offered: &PublicKey) -> Option<HostKeyQuestion> {
        let mut recorded = Vec::new();
        let mut unread = Vec::new();
        for (path, whose) in self.files() {
            match known_host_keys_path(host, port, path) {
                Ok(found) => recorded.extend(found.into_iter().map(|(_, key)| key)),
                Err(_) => unread.push(whose),
            }
        }

        if recorded.iter().any(|key| key == offered) {
            return None;
        }

        let state = recorded
            .iter()
            .find(|key| key.algorithm() == offered.algorithm())
            .map_or(HostKeyState::Unknown, |key| HostKeyState::Changed {
                recorded: fingerprint(key),
            });

        Some(HostKeyQuestion {
            host: host.to_owned(),
            port,
            fingerprint: fingerprint(offered),
            state,
            aside: aside(&unread),
        })
    }

    /// Writes an accepted key into Acter's own file, so the same host is not asked about
    /// again.
    ///
    /// **Only ever this file.** The user's `known_hosts` is not touched here or anywhere
    /// else, which is decision 5's second half and the reason a mistake in this code cannot
    /// cost somebody their `ssh` configuration.
    ///
    /// The error is a whole spoken sentence and it says what the *consequence* is rather
    /// than only what failed: a key that could not be written down means being asked again
    /// next time, and a user who is told that will not think the question is a fault.
    pub fn remember(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), String> {
        learn_known_hosts_path(host, port, key, &self.ours).map_err(|why| {
            ended(format!(
                "Acter accepted the host key for {host} but could not write it down, so \
                 this host will be asked about again next time. {why}"
            ))
        })
    }

    /// Which key algorithms are already recorded for this host, in the order they were
    /// found.
    ///
    /// **This is what stops a host the user knows from being asked about anyway.** A server
    /// usually offers several kinds of key and the client picks; if the client picks one
    /// the user has no record of, a perfectly familiar host arrives as an unknown one. So
    /// the algorithms already on file are offered to the server first, which is what `ssh`
    /// does with its own `known_hosts` and the reason it does not prompt on every second
    /// connection.
    pub fn recorded_algorithms(&self, host: &str, port: u16) -> Vec<Algorithm> {
        let mut found: Vec<Algorithm> = Vec::new();
        for (path, _) in self.files() {
            let Ok(keys) = known_host_keys_path(host, port, path) else {
                continue;
            };
            for (_, key) in keys {
                let algorithm = key.algorithm();
                if !found.contains(&algorithm) {
                    found.push(algorithm);
                }
            }
        }
        found
    }

    /// The files to consult, Acter's own first — so a key Acter itself accepted is the one
    /// reported as recorded when both files hold something for the same host.
    fn files(&self) -> Vec<(&Path, &'static str)> {
        let mut files: Vec<(&Path, &'static str)> = vec![(self.ours.as_path(), OURS)];
        files.extend(self.theirs.as_deref().map(|path| (path, THEIRS)));
        files
    }
}

/// A key as `ssh-keygen -l` prints it: `SHA256:` and unpadded base64.
///
/// **The form is not a presentation choice.** What a listener compares this against is
/// what their hosting provider printed, what a colleague read to them, or what
/// `docker logs` showed — and every one of those is `ssh-keygen`'s spelling. A prettier
/// rendering would be a rendering nobody else produces.
fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// What to add to a question when a file could not be read, and nothing when both were.
fn aside(unread: &[&str]) -> Option<String> {
    match unread {
        [] => None,
        [one] => Some(format!(
            "Acter could not read {one}, so a key recorded only there was not compared."
        )),
        [first, second, ..] => Some(format!(
            "Acter could not read {first} or {second}, so no recorded key was compared."
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use russh::keys::parse_public_key_base64;

    use super::*;

    /// Three real keys, generated once with `ssh-keygen` and pinned here.
    ///
    /// **Fixed rather than generated per run**, because two of these tests are about two
    /// keys being *different* and one is about a hashed host line computed from a
    /// particular key; a fresh key every run would make the hashed line meaningless and the
    /// failures irreproducible.
    const KEY_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIHKQ43TBPmSEIjzocj1VrRSKA4Vxa65wu0uNWQx49Tfk";
    const KEY_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIDe0jh7xXi53Y3S8vM15MCYD+zTOLfCzhQCPCkziiyZM";
    const KEY_ECDSA: &str = "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBKurhONORqK9uvgD9m\
                             aATfjsyMBYDY04eG0WXmzbkN3AKvzd1HDqGLib0zksHzow5oTVlV+Yrljc3qkLt86pvYk=";

    /// What `ssh-keygen -l` says about `KEY_A`, so the fingerprint this module produces is
    /// pinned against the tool a user will compare it with rather than against itself.
    const FINGERPRINT_A: &str = "SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU";

    /// `[127.0.0.1]:2222` and `KEY_A`, hashed by `ssh-keygen -H` — the shape of every line
    /// in the `known_hosts` of anybody who has `HashKnownHosts` turned on, which on most
    /// distributions is everybody.
    const HASHED_A: &str = "|1|Ha/HDSIabMJpub+892dhUsL3Z2Y=|CNDgL5MIPMFgJR0LY+Gc73S2wK8= \
                            ssh-ed25519 \
                            AAAAC3NzaC1lZDI1NTE5AAAAIHKQ43TBPmSEIjzocj1VrRSKA4Vxa65wu0uNWQx49Tfk";

    const HOST: &str = "127.0.0.1";
    const PORT: u16 = 2222;

    fn key(base64: &str) -> PublicKey {
        parse_public_key_base64(base64).expect("a pinned key parses")
    }

    /// A directory of this test's own, removed when it goes.
    ///
    /// Written here rather than taken from a crate, because this workspace already builds
    /// its temporary paths this way (`tests/real_session.rs`) and one test dependency for
    /// fifteen lines is a dependency to keep updated forever.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NONCE: AtomicUsize = AtomicUsize::new(0);
            let unique = NONCE.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("acter-b9-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("a scratch directory is made");
            Self(path)
        }

        /// A file in it, holding these lines.
        fn file(&self, name: &str, lines: &[&str]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, format!("{}\n", lines.join("\n"))).expect("a fixture is written");
            path
        }

        /// A path in it that nothing has created.
        fn missing(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// One line of a `known_hosts` file, spelled as OpenSSH spells a non-standard port.
    fn line(base64: &str) -> String {
        let algorithm = if base64 == KEY_ECDSA {
            "ecdsa-sha2-nistp256"
        } else {
            "ssh-ed25519"
        };
        format!("[{HOST}]:{PORT} {algorithm} {base64}")
    }

    /// Nothing recorded anywhere: the routine first connection, and the one case where
    /// every user meets this dialog.
    #[test]
    fn a_host_nobody_has_recorded_is_unknown() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(scratch.missing("acter_known_hosts"), None);

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        assert_eq!(question.state, HostKeyState::Unknown);
        assert_eq!(question.host, HOST);
        assert_eq!(question.port, PORT);
        assert_eq!(
            question.fingerprint, FINGERPRINT_A,
            "the fingerprint is what ssh-keygen -l would print"
        );
        assert_eq!(
            question.aside, None,
            "a file that was never created is not a file that could not be read"
        );
    }

    /// The key Acter itself accepted last time: no question at all.
    #[test]
    fn a_key_acter_recorded_is_not_asked_about_again() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(scratch.file("acter_known_hosts", &[&line(KEY_A)]), None);

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    /// **The reason the user's own file is read at all** (decision 5): a host they trust
    /// connects without being asked, so a populated `known_hosts` does not become a
    /// sequence of prompts.
    #[test]
    fn a_key_the_user_already_trusts_is_not_asked_about() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            scratch.missing("acter_known_hosts"),
            Some(scratch.file("known_hosts", &[&line(KEY_A)])),
        );

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    /// **Even when it is hashed**, which is the shape most people's file is actually in.
    /// Without this, `HashKnownHosts yes` would silently turn every trusted host back into
    /// an unknown one and the previous test would have proved nothing about real machines.
    #[test]
    fn a_hashed_entry_matches_the_host_it_was_hashed_from() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            scratch.missing("acter_known_hosts"),
            Some(scratch.file("known_hosts", &[HASHED_A])),
        );

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    /// The security case: something is recorded, the same kind of key, and it is not this
    /// one. Both fingerprints travel, because the two have to be readable one after the
    /// other for a person to compare them.
    #[test]
    fn a_different_key_of_the_same_kind_is_a_changed_key() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(scratch.file("acter_known_hosts", &[&line(KEY_B)]), None);

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        let HostKeyState::Changed { recorded } = &question.state else {
            panic!("a key that was recorded differently has changed: {question:?}");
        };
        assert_ne!(recorded, &question.fingerprint);
        assert_eq!(question.fingerprint, FINGERPRINT_A, "the offered key");
    }

    /// A server that grew a second kind of key has not changed identity, and `ssh` does not
    /// say it has. Spending the alarming sentence on this would teach a user to ignore it.
    #[test]
    fn a_key_of_a_kind_nobody_recorded_is_unknown_rather_than_changed() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(scratch.file("acter_known_hosts", &[&line(KEY_ECDSA)]), None);

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        assert_eq!(question.state, HostKeyState::Unknown);
    }

    /// The port is part of the identity, exactly as it is for OpenSSH: the rig runs on
    /// 2222 and a different server may well be on 22 of the same machine.
    #[test]
    fn the_same_host_on_another_port_is_another_host() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(scratch.file("acter_known_hosts", &[&line(KEY_A)]), None);

        let question = hosts
            .check(HOST, 22, &key(KEY_A))
            .expect("port 22 was never recorded");

        assert_eq!(question.state, HostKeyState::Unknown);
    }

    /// Accepting writes to Acter's file, creating it, and the same key is then silent.
    #[test]
    fn an_accepted_key_is_written_down_and_then_stops_asking() {
        let scratch = Scratch::new();
        let ours = scratch.missing("acter_known_hosts");
        let hosts = KnownHosts::new(ours.clone(), None);

        hosts
            .remember(HOST, PORT, &key(KEY_A))
            .expect("a fresh file is written");

        assert!(ours.exists(), "the file is created rather than required");
        assert_eq!(
            hosts.check(HOST, PORT, &key(KEY_A)),
            None,
            "what was just accepted is not asked about again"
        );
    }

    /// **The half of decision 5 that a bug here could take from somebody**: the user's own
    /// `known_hosts` is never written, not when a key is checked and not when one is
    /// accepted. Asserted on the bytes, so any write at all fails this.
    #[test]
    fn the_users_own_file_is_never_written() {
        let scratch = Scratch::new();
        let theirs = scratch.file("known_hosts", &[&line(KEY_B)]);
        let before = fs::read(&theirs).expect("the fixture is readable");
        let hosts = KnownHosts::new(scratch.missing("acter_known_hosts"), Some(theirs.clone()));

        hosts.check(HOST, PORT, &key(KEY_A));
        hosts
            .remember(HOST, PORT, &key(KEY_A))
            .expect("accepting writes Acter's own file");

        assert_eq!(
            fs::read(&theirs).expect("it is still readable"),
            before,
            "nothing Acter does touches the user's own known_hosts"
        );
    }

    /// A file Acter cannot make sense of is not a connection that fails. The user is asked,
    /// and told that the comparison did not happen — which is the difference between a
    /// question they can answer and a question they cannot account for.
    #[test]
    fn a_file_that_cannot_be_read_is_an_aside_rather_than_a_failure() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            scratch.missing("acter_known_hosts"),
            Some(scratch.file(
                "known_hosts",
                &[&format!("[{HOST}]:{PORT} ssh-ed25519 not-base64")],
            )),
        );

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        assert_eq!(question.state, HostKeyState::Unknown);
        let aside = question
            .aside
            .expect("the user is told the comparison did not happen");
        assert!(aside.contains(THEIRS), "it names whose file: {aside}");
        assert!(aside.ends_with('.'), "it is a sentence: {aside}");
    }

    /// What the transport offers the server first, so a host the user knows is not asked
    /// about merely because the client and the server agreed on a different kind of key.
    #[test]
    fn the_algorithms_already_on_file_are_reported_for_the_server_to_be_offered() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            scratch.file("acter_known_hosts", &[&line(KEY_ECDSA)]),
            Some(scratch.file("known_hosts", &[&line(KEY_A)])),
        );

        let algorithms = hosts.recorded_algorithms(HOST, PORT);

        assert_eq!(
            algorithms,
            vec![
                Algorithm::Ecdsa {
                    curve: russh::keys::EcdsaCurve::NistP256
                },
                Algorithm::Ed25519
            ],
            "Acter's own record first, then the user's, each kind once"
        );
        assert!(
            hosts.recorded_algorithms("no-such-host", PORT).is_empty(),
            "a host nobody recorded constrains nothing"
        );
    }
}
