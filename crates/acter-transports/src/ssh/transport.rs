//! Adapter: [`SshTransport`] — a session on a machine that is not this one, behind
//! acter-core's [`Transport`] port.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use acter_core::{
    HostKeyAnswer, HostKeyState, PasswordQuestion, SshQuestions, Transport, TransportError, ended,
};
use russh::client::{self, Msg};
use russh::keys::PublicKey;
use russh::{ChannelMsg, Preferred};
use russh::{ChannelReadHalf, ChannelWriteHalf, Sig};
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::ssh::KnownHosts;
use crate::ssh::probe::{self, FarEnd};

const TERM: &str = "xterm-256color";

const INTERRUPT: u8 = 0x03;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
}

pub struct SshTransport {
    /// Dropping it ends the session: the pump exits and drops the connection it holds.
    outgoing: UnboundedSender<Outgoing>,
    ready: Option<Ready>,
    far_end: FarEnd,
}

struct Ready {
    read: ChannelReadHalf,
    write: ChannelWriteHalf<Msg>,
    inbox: UnboundedReceiver<Outgoing>,
    connection: client::Handle<Verifier>,
}

/// One queue, so a resize never overtakes the write before it.
enum Outgoing {
    Data(Vec<u8>),
    Interrupt,
    Resize { columns: u16, screen_lines: u16 },
}

impl SshTransport {
    /// Err is a whole spoken sentence.
    pub async fn connect(
        target: &SshTarget,
        hosts: Arc<KnownHosts>,
        questions: Arc<dyn SshQuestions>,
        columns: u16,
        screen_lines: u16,
        patience: Duration,
    ) -> Result<Self, String> {
        let SshTarget { host, port, user } = target;
        let (host, port, user) = (host.as_str(), *port, user.as_str());
        questions.tell(&format!("Connecting to {host}."));

        let refusal = Arc::new(Mutex::new(None));
        let verifier = Verifier {
            hosts: Arc::clone(&hosts),
            questions: Arc::clone(&questions),
            host: host.to_owned(),
            port,
            refusal: Arc::clone(&refusal),
        };

        let mut config = client::Config::default();
        let recorded = hosts.recorded_algorithms(host, port);
        if !recorded.is_empty() {
            let mut preferred = Preferred::DEFAULT.key.to_vec();
            preferred.retain(|algorithm| !recorded.contains(algorithm));
            config.preferred.key = [recorded, preferred].concat().into();
        }

        let mut connection = client::connect(Arc::new(config), (host, port), verifier)
            .await
            .map_err(|why| {
                refusal
                    .lock()
                    .expect("refusal lock poisoned")
                    .take()
                    .unwrap_or_else(|| {
                        ended(format!(
                            "Acter could not reach {host} on port {port}. {why}"
                        ))
                    })
            })?;
        if let Some(refused) = refusal.lock().expect("refusal lock poisoned").take() {
            return Err(refused);
        }

        authenticate(&mut connection, host, user, &questions).await?;

        let far_end = probe::ask(&mut connection, patience).await;

        questions.tell("Opening a shell.");
        let channel = connection.channel_open_session().await.map_err(|why| {
            ended(format!(
                "Acter signed in to {host} but could not open a session on it. {why}"
            ))
        })?;
        channel
            .request_pty(
                true,
                TERM,
                u32::from(columns),
                u32::from(screen_lines),
                0,
                0,
                &[],
            )
            .await
            .map_err(|why| {
                ended(format!(
                    "The server at {host} would not give Acter a terminal to run the shell \
                     in. {why}"
                ))
            })?;
        channel.request_shell(true).await.map_err(|why| {
            ended(format!(
                "The server at {host} would not start a shell. {why}"
            ))
        })?;

        let (read, write) = channel.split();
        let (outgoing, inbox) = unbounded_channel();
        Ok(Self {
            outgoing,
            ready: Some(Ready {
                read,
                write,
                inbox,
                connection,
            }),
            far_end,
        })
    }

    pub fn far_end(&self) -> &FarEnd {
        &self.far_end
    }

    fn ask(&self, outgoing: Outgoing) -> Result<(), TransportError> {
        if self.ready.is_some() {
            return Err(TransportError::NotStarted);
        }
        self.outgoing
            .send(outgoing)
            .map_err(|_| TransportError::Closed)
    }
}

impl Transport for SshTransport {
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let Some(Ready {
            mut read,
            write,
            mut inbox,
            connection,
        }) = self.ready.take()
        else {
            return;
        };

        tokio::spawn(async move {
            // Held for the pump's lifetime: dropping it closes the connection.
            let _connection = connection;
            loop {
                tokio::select! {
                    message = read.wait() => match message {
                        // `ExtendedData` is the far end's standard error.
                        Some(ChannelMsg::Data { data })
                        | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if bytes.send(data.to_vec()).await.is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    },
                    request = inbox.recv() => match request {
                        Some(Outgoing::Data(data)) => {
                            if write.data_bytes(data).await.is_err() {
                                break;
                            }
                        }
                        Some(Outgoing::Interrupt) => {
                            // A remote pty's line discipline turns the byte into
                            // `SIGINT`; the signal request is for a server that acts on one.
                            let _ = write.data_bytes(vec![INTERRUPT]).await;
                            let _ = write.signal(Sig::INT).await;
                        }
                        Some(Outgoing::Resize { columns, screen_lines }) => {
                            let _ = write
                                .window_change(u32::from(columns), u32::from(screen_lines), 0, 0)
                                .await;
                        }
                        None => break,
                    },
                }
            }
        });
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.ask(Outgoing::Data(bytes.to_vec()))
    }

    fn interrupt(&mut self) -> Result<(), TransportError> {
        self.ask(Outgoing::Interrupt)
    }

    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        self.ask(Outgoing::Resize {
            columns,
            screen_lines,
        })
    }
}

async fn authenticate(
    connection: &mut client::Handle<Verifier>,
    host: &str,
    user: &str,
    questions: &Arc<dyn SshQuestions>,
) -> Result<(), String> {
    questions.tell("Signing in.");
    let mut again = false;
    loop {
        let question = PasswordQuestion {
            host: host.to_owned(),
            user: user.to_owned(),
            again,
        };
        let Some(secret) = ask(questions, move |questions| questions.password(question)).await
        else {
            return Err(format!(
                "Acter did not sign in to {host}, because no password was given."
            ));
        };

        let result = connection
            .authenticate_password(user, secret.expose())
            .await
            .map_err(|why| lost_while_asking(host, &why))?;
        if result.success() {
            return Ok(());
        }
        if !offers_password(&result) {
            return Err(format!(
                "The server at {host} would not accept that password for {user}, and will \
                 not accept another one. Check the account name and the password, then try \
                 again."
            ));
        }
        again = true;
    }
}

/// OpenSSH's `LoginGraceTime`, two minutes by default, runs while the user is answering,
/// and russh reports the server hanging up then as `SendError`.
fn lost_while_asking(host: &str, why: &russh::Error) -> String {
    if matches!(why, russh::Error::SendError) {
        return format!(
            "The server at {host} closed the connection before signing in finished. Servers \
             usually allow about two minutes to sign in, and that time ran out. Connect \
             again, and have the password ready."
        );
    }
    ended(format!("Acter could not sign in to {host}. {why}"))
}

fn offers_password(result: &russh::client::AuthResult) -> bool {
    match result {
        russh::client::AuthResult::Success => false,
        russh::client::AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods.contains(&russh::MethodKind::Password),
    }
}

/// Nothing that waits for a person may hold a runtime worker.
async fn ask<T, F>(questions: &Arc<dyn SshQuestions>, question: F) -> T
where
    T: Send + 'static,
    F: FnOnce(&dyn SshQuestions) -> T + Send + 'static,
{
    let questions = Arc::clone(questions);
    tokio::task::spawn_blocking(move || question(questions.as_ref()))
        .await
        .expect("asking a question does not panic")
}

#[derive(Clone)]
struct Verifier {
    hosts: Arc<KnownHosts>,
    questions: Arc<dyn SshQuestions>,
    host: String,
    port: u16,
    /// `Some` when a person refused, read after `connect` fails.
    refusal: Arc<Mutex<Option<String>>>,
}

impl Verifier {
    fn refuse(&self, why: String) {
        *self.refusal.lock().expect("refusal lock poisoned") = Some(why);
    }

    fn decide(&self, key: &PublicKey) -> bool {
        let Some(question) = self.hosts.check(&self.host, self.port, key) else {
            return true;
        };

        let changed = matches!(question.state, HostKeyState::Changed { .. });
        if self.questions.host_key(question) == HostKeyAnswer::Refuse {
            self.refuse(if changed {
                format!(
                    "Acter did not connect to {}, because you did not accept its changed \
                     host key.",
                    self.host
                )
            } else {
                format!(
                    "Acter did not connect to {}, because you did not accept its host key.",
                    self.host
                )
            });
            return false;
        }

        if let Err(why) = self.hosts.remember(&self.host, self.port, key) {
            self.questions.tell(&why);
        }
        true
    }
}

impl client::Handler for Verifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        offered: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match offered {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            russh::keys::PublicKeyOrCertificate::Certificate(_) => {
                self.refuse(format!(
                    "The server at {} identified itself with a certificate, which Acter \
                     cannot check yet. Acter did not connect.",
                    self.host
                ));
                return Ok(false);
            }
        };

        let verifier = self.clone();
        Ok(tokio::task::spawn_blocking(move || verifier.decide(&key))
            .await
            .unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_that_went_away_while_asking_says_what_to_do_about_it() {
        let said = lost_while_asking("acter-ssh", &russh::Error::SendError);

        assert!(
            said.contains("two minutes"),
            "it names the deadline, which is the one thing that makes the next attempt \
             work: {said}"
        );
        assert!(
            said.contains("Connect again"),
            "and says what to do: {said}"
        );
        assert!(said.ends_with('.'), "a spoken message ends: {said}");
        assert!(
            !said.contains("Channel send error"),
            "and never repeats the library's own words: {said}"
        );
    }

    #[test]
    fn any_other_failure_keeps_the_reason_the_world_gave() {
        let said = lost_while_asking("acter-ssh", &russh::Error::Disconnect);

        assert!(
            said.starts_with("Acter could not sign in to acter-ssh."),
            "{said}"
        );
        assert!(said.ends_with('.'), "a spoken message ends: {said}");
    }
}
