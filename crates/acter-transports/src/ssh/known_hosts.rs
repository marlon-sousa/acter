//! Adapter: what Acter knows about a server's identity — which host keys have been seen
//! before, which one changed, and where a newly accepted one is written down.
//!
//! The user's own `known_hosts` is read and never written; what Acter accepts goes to the
//! settings document through [`HostKeyStore`].

use std::path::PathBuf;
use std::sync::Arc;

use acter_core::{
    AcceptedHostKey, HostKeyOrigin, HostKeyQuestion, HostKeyState, HostKeyStore, ended,
};
use russh::keys::known_hosts::known_host_keys_path;
use russh::keys::{Algorithm, HashAlg, PublicKey};

pub struct KnownHosts {
    ours: Arc<dyn HostKeyStore>,
    /// `None` when there is no home directory to look in, which is not an error.
    theirs: Option<PathBuf>,
}

const THEIRS: &str = "your own OpenSSH known hosts file";

impl KnownHosts {
    pub fn new(ours: Arc<dyn HostKeyStore>, theirs: Option<PathBuf>) -> Self {
        Self { ours, theirs }
    }

    /// `None` when either record already holds this key.
    pub fn check(&self, host: &str, port: u16, offered: &PublicKey) -> Option<HostKeyQuestion> {
        let mut unread = None;
        let recorded = self.known(host, port, &mut unread);
        let algorithm = offered.algorithm().to_string();
        let offered = fingerprint(offered);

        if recorded
            .iter()
            .any(|key| key.algorithm == algorithm && key.fingerprint == offered)
        {
            return None;
        }

        let state = recorded
            .iter()
            .find(|key| key.algorithm == algorithm)
            .map_or(HostKeyState::Unknown, |key| HostKeyState::Changed {
                recorded: key.fingerprint.clone(),
            });

        Some(HostKeyQuestion {
            host: host.to_owned(),
            port,
            fingerprint: offered,
            state,
            aside: aside(unread),
        })
    }

    /// Acter's own rows come first, so its key is the one reported as recorded when both
    /// hold one; `unread` names a record that could not be read.
    pub fn known(
        &self,
        host: &str,
        port: u16,
        unread: &mut Option<&'static str>,
    ) -> Vec<AcceptedHostKey> {
        let mut found: Vec<AcceptedHostKey> = self
            .ours
            .accepted()
            .into_iter()
            .filter(|had| had.is_for(host, port))
            .collect();
        if let Some(theirs) = self.theirs.as_deref() {
            match known_host_keys_path(host, port, theirs) {
                Ok(keys) => found.extend(keys.into_iter().map(|(_, key)| AcceptedHostKey {
                    host: host.to_owned(),
                    port,
                    algorithm: key.algorithm().as_ref().to_owned(),
                    fingerprint: fingerprint(&key),
                    // Empty: `ssh` does not date its own file.
                    accepted: String::new(),
                    origin: HostKeyOrigin::Native,
                })),
                Err(_) => *unread = Some(THEIRS),
            }
        }
        found
    }

    /// Err is a whole spoken sentence.
    pub fn remember(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), String> {
        self.ours
            .accept(host, port, key.algorithm().as_ref(), &fingerprint(key))
            .map_err(|why| {
                ended(format!(
                    "Acter accepted the host key for {host} but could not write it down, so \
                     this host will be asked about again next time. {why}"
                ))
            })
    }

    pub fn recorded_algorithms(&self, host: &str, port: u16) -> Vec<Algorithm> {
        let mut found: Vec<Algorithm> = Vec::new();
        for recorded in self.known(host, port, &mut None) {
            let Ok(algorithm) = Algorithm::new(&recorded.algorithm) else {
                continue;
            };
            if !found.contains(&algorithm) {
                found.push(algorithm);
            }
        }
        found
    }
}

/// A key as `ssh-keygen -l` prints it: `SHA256:` and unpadded base64.
fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

fn aside(unread: Option<&str>) -> Option<String> {
    unread.map(|whose| {
        format!("Acter could not read {whose}, so a key recorded only there was not compared.")
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use acter_core::RememberedHostKeys;
    use russh::keys::parse_public_key_base64;

    use super::*;

    const KEY_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIHKQ43TBPmSEIjzocj1VrRSKA4Vxa65wu0uNWQx49Tfk";
    const KEY_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIDe0jh7xXi53Y3S8vM15MCYD+zTOLfCzhQCPCkziiyZM";
    const KEY_ECDSA: &str = "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBKurhONORqK9uvgD9m\
                             aATfjsyMBYDY04eG0WXmzbkN3AKvzd1HDqGLib0zksHzow5oTVlV+Yrljc3qkLt86pvYk=";

    /// What `ssh-keygen -l` prints for `KEY_A`.
    const FINGERPRINT_A: &str = "SHA256:IzJE9oHP7rabiNsCSTceP2l1jW8/4WESW2jkk+JFiOU";

    fn ours() -> Arc<RememberedHostKeys> {
        Arc::new(RememberedHostKeys::default())
    }

    fn ours_holding(base64: &str) -> Arc<RememberedHostKeys> {
        let recorded = RememberedHostKeys::default();
        let key = key(base64);
        recorded
            .accept(HOST, PORT, key.algorithm().as_ref(), &fingerprint(&key))
            .expect("a fixture is written");
        Arc::new(recorded)
    }

    /// `[127.0.0.1]:2222` and `KEY_A`, hashed by `ssh-keygen -H`.
    const HASHED_A: &str = "|1|Ha/HDSIabMJpub+892dhUsL3Z2Y=|CNDgL5MIPMFgJR0LY+Gc73S2wK8= \
                            ssh-ed25519 \
                            AAAAC3NzaC1lZDI1NTE5AAAAIHKQ43TBPmSEIjzocj1VrRSKA4Vxa65wu0uNWQx49Tfk";

    const HOST: &str = "127.0.0.1";
    const PORT: u16 = 2222;

    fn key(base64: &str) -> PublicKey {
        parse_public_key_base64(base64).expect("a pinned key parses")
    }

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

        fn file(&self, name: &str, lines: &[&str]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, format!("{}\n", lines.join("\n"))).expect("a fixture is written");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn line(base64: &str) -> String {
        let algorithm = if base64 == KEY_ECDSA {
            "ecdsa-sha2-nistp256"
        } else {
            "ssh-ed25519"
        };
        format!("[{HOST}]:{PORT} {algorithm} {base64}")
    }

    #[test]
    fn a_host_nobody_has_recorded_is_unknown() {
        let hosts = KnownHosts::new(ours(), None);

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

    #[test]
    fn a_key_acter_recorded_is_not_asked_about_again() {
        let hosts = KnownHosts::new(ours_holding(KEY_A), None);

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    #[test]
    fn a_key_the_user_already_trusts_is_not_asked_about() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(ours(), Some(scratch.file("known_hosts", &[&line(KEY_A)])));

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    #[test]
    fn a_hashed_entry_matches_the_host_it_was_hashed_from() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(ours(), Some(scratch.file("known_hosts", &[HASHED_A])));

        assert_eq!(hosts.check(HOST, PORT, &key(KEY_A)), None);
    }

    #[test]
    fn a_different_key_of_the_same_kind_is_a_changed_key() {
        let hosts = KnownHosts::new(ours_holding(KEY_B), None);

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        let HostKeyState::Changed { recorded } = &question.state else {
            panic!("a key that was recorded differently has changed: {question:?}");
        };
        assert_ne!(recorded, &question.fingerprint);
        assert_eq!(question.fingerprint, FINGERPRINT_A, "the offered key");
    }

    #[test]
    fn a_key_of_a_kind_nobody_recorded_is_unknown_rather_than_changed() {
        let hosts = KnownHosts::new(ours_holding(KEY_ECDSA), None);

        let question = hosts
            .check(HOST, PORT, &key(KEY_A))
            .expect("it is asked about");

        assert_eq!(question.state, HostKeyState::Unknown);
    }

    #[test]
    fn the_same_host_on_another_port_is_another_host() {
        let hosts = KnownHosts::new(ours_holding(KEY_A), None);

        let question = hosts
            .check(HOST, 22, &key(KEY_A))
            .expect("port 22 was never recorded");

        assert_eq!(question.state, HostKeyState::Unknown);
    }

    #[test]
    fn an_accepted_key_is_written_down_and_then_stops_asking() {
        let ours = ours();
        let hosts = KnownHosts::new(Arc::clone(&ours) as Arc<dyn HostKeyStore>, None);

        hosts
            .remember(HOST, PORT, &key(KEY_A))
            .expect("the record takes it");

        let kept = ours.accepted();
        assert_eq!(kept.len(), 1, "one record, and it is Acter's own");
        assert!(kept[0].is_acters_own());
        assert_eq!(kept[0].fingerprint, FINGERPRINT_A);
        assert_eq!(
            hosts.check(HOST, PORT, &key(KEY_A)),
            None,
            "what was just accepted is not asked about again"
        );
    }

    #[test]
    fn what_is_known_is_one_list_saying_which_half_each_row_came_from() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            ours_holding(KEY_B),
            Some(scratch.file("known_hosts", &[&line(KEY_A)])),
        );

        let known = hosts.known(HOST, PORT, &mut None);

        assert_eq!(known.len(), 2, "both records, in one list");
        assert!(known[0].is_acters_own(), "Acter's own comes first");
        assert!(!known[1].is_acters_own(), "and the user's own is native");
        assert_eq!(known[1].fingerprint, FINGERPRINT_A);
        assert!(
            known[1].accepted.is_empty(),
            "ssh does not date its file, and Acter does not invent a day for it"
        );
    }

    #[test]
    fn nothing_read_out_of_the_users_file_is_ever_written_into_acters_record() {
        let scratch = Scratch::new();
        let ours = ours();
        let hosts = KnownHosts::new(
            Arc::clone(&ours) as Arc<dyn HostKeyStore>,
            Some(scratch.file("known_hosts", &[&line(KEY_A)])),
        );

        hosts.check(HOST, PORT, &key(KEY_ECDSA));
        hosts
            .remember(HOST, PORT, &key(KEY_ECDSA))
            .expect("the record takes it");

        let kept = ours.accepted();
        assert_eq!(kept.len(), 1, "only the one Acter was told to keep");
        assert!(kept.iter().all(|had| had.is_acters_own()));
        assert!(
            kept.iter().all(|had| had.fingerprint != FINGERPRINT_A),
            "the key that was only ever in the user's file stayed there"
        );
    }

    #[test]
    fn the_users_own_file_is_never_written() {
        let scratch = Scratch::new();
        let theirs = scratch.file("known_hosts", &[&line(KEY_B)]);
        let before = fs::read(&theirs).expect("the fixture is readable");
        let hosts = KnownHosts::new(ours(), Some(theirs.clone()));

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

    #[test]
    fn a_file_that_cannot_be_read_is_an_aside_rather_than_a_failure() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            ours(),
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

    #[test]
    fn the_algorithms_already_on_file_are_reported_for_the_server_to_be_offered() {
        let scratch = Scratch::new();
        let hosts = KnownHosts::new(
            ours_holding(KEY_ECDSA),
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
