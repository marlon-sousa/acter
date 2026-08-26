//! Adapter: [`SshTransport`] — a session on a machine that is not this one, behind
//! acter-core's [`Transport`] port.
//!
//! **The third implementer of a seam that has not moved since B3.5.** Everything above it
//! — the engine, the boundary tracker, the correlation, the pacing policy — has run over a
//! scripted far end and over a local pseudoconsole, and does not change because these bytes
//! crossed a network. Two things the port already modelled turn out to be right here, which
//! is the evidence the seam was cut in the right place: `interrupt` and `resize` are
//! methods rather than bytes a caller computes, because over a connection both are things
//! done to a channel rather than text written into one.
//!
//! **Connecting is async and starting is not, and that is deliberate.** [`Transport::start`]
//! is sync because a `LocalPty` starts by spawning a process; an SSH session is *already
//! established* by the time anything above it exists, because establishing it is where the
//! questions live. So [`SshTransport::connect`] is an `async fn` that returns a transport
//! whose channel is open, and `start` only spawns the pump that carries bytes both ways.
//! Nothing here ever blocks a thread waiting for a person: the questions are asked through
//! a port on a blocking task of their own, so a dialog on screen never has a runtime worker
//! parked behind it (spec B9).
//!
//! **The session is unintegrated and this file does nothing to change that** (spec B9,
//! decision 2). No environment is injected, because there is no carrier: measured against
//! the rig, `PROMPT_COMMAND` sent with `SendEnv` arrives empty, since OpenSSH's stock
//! `AcceptEnv` is `LANG` and `LC_*` and a server belonging to somebody else has not been
//! configured for us. What the far end runs is the account's own login shell, exactly as
//! `ssh` would start it.

use std::sync::{Arc, Mutex};

use acter_core::{
    HostKeyAnswer, HostKeyState, PasswordQuestion, SshQuestions, Transport, TransportError, ended,
};
use russh::client::{self, Msg};
use russh::keys::PublicKey;
use russh::{ChannelMsg, Preferred};
use russh::{ChannelReadHalf, ChannelWriteHalf, Sig};
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::ssh::KnownHosts;

/// What the far end is told this terminal is.
///
/// **`xterm-256color`, which is what the emulator above this actually is.** The engine is
/// `alacritty_terminal`, and a remote program choosing its escape sequences from `TERM`
/// has to be choosing the ones that engine parses. Announcing something smaller would make
/// a far end degrade its output for a terminal Acter is not.
const TERM: &str = "xterm-256color";

/// End of text — what a pseudoconsole turns into an interrupt for the process attached to
/// it, and what a remote pty's line discipline does with it too.
///
/// **Measured against the rig rather than assumed** (see `tests/ssh_rig.rs`). Spec B9 was
/// drafted saying an interrupt over SSH is a channel request, which is what the protocol
/// offers; what OpenSSH's `sshd` actually does with a `signal` request on an interactive
/// session is another matter, and the honest answer is the one the far end responds to.
/// Both are sent, cheapest first: the byte is what the remote line discipline turns into
/// `SIGINT`, and the request is what a server that implements it would act on.
const INTERRUPT: u8 = 0x03;

/// One session over one SSH connection.
pub struct SshTransport {
    /// What the pump is told to do. Dropping it is what ends the session: the pump's
    /// receive returns `None`, the pump breaks out of its loop, and the connection handle
    /// it owns is dropped with it — which is what closes the connection.
    outgoing: UnboundedSender<Outgoing>,
    /// Taken by [`Transport::start`], which is what makes starting twice a no-op rather
    /// than two pumps racing over one channel.
    ready: Option<Ready>,
}

/// Everything the pump needs, held between connecting and starting.
struct Ready {
    read: ChannelReadHalf,
    write: ChannelWriteHalf<Msg>,
    inbox: UnboundedReceiver<Outgoing>,
    /// Kept only so it lives as long as the session: dropping the handle drops the
    /// connection, and a channel whose connection has gone answers nothing.
    connection: client::Handle<Verifier>,
}

/// What a sync caller above asks of the async connection below.
///
/// **A message rather than a method call**, because [`Transport`] is sync by design and
/// everything russh offers is async. One queue keeps them ordered: a resize that overtook
/// the write before it would tell the far end about a screen the output was not written
/// for.
enum Outgoing {
    Data(Vec<u8>),
    Interrupt,
    Resize { columns: u16, screen_lines: u16 },
}

impl SshTransport {
    /// Opens a connection, answers whatever it asks, and starts a shell on it.
    ///
    /// The error is a whole spoken sentence, because it reaches somebody who has just
    /// filled in a form and is waiting to hear what happened (CLAUDE.md).
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        hosts: Arc<KnownHosts>,
        questions: Arc<dyn SshQuestions>,
        columns: u16,
        screen_lines: u16,
    ) -> Result<Self, String> {
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
        // **What the user already trusts is offered first.** A server usually has several
        // kinds of host key; if the negotiation picks one this machine has no record of,
        // a host the user knows perfectly well arrives as an unknown one and they are asked
        // about a server they have used for years. This is what `ssh` does with its own
        // `known_hosts`, and the reason it does not prompt every second connection.
        let recorded = hosts.recorded_algorithms(host, port);
        if !recorded.is_empty() {
            let mut preferred = Preferred::DEFAULT.key.to_vec();
            preferred.retain(|algorithm| !recorded.contains(algorithm));
            config.preferred.key = [recorded, preferred].concat().into();
        }

        let mut connection = client::connect(Arc::new(config), (host, port), verifier)
            .await
            .map_err(|why| {
                // A key the user refused is not a network failure, and saying "the
                // connection failed" for it would hide the one thing they need to know:
                // that Acter did what they told it to.
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
        })
    }

    /// Queues one thing for the pump, saying the session has ended if it is no longer
    /// there to do it.
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
    /// Starts the pump: one task carrying the far end's bytes up and everything above's
    /// requests down.
    ///
    /// **One send is one read**, as the port requires. What arrives as one `Data` message
    /// is delivered as one `Vec<u8>` and never merged with the next, because a marker split
    /// across two reads is exactly the case the domain above has to survive — and over a
    /// network that split is not hypothetical.
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
            // Named so it is obvious that the connection is held for the pump's lifetime:
            // dropping it closes the connection, and that is how a session ends.
            let _connection = connection;
            loop {
                tokio::select! {
                    message = read.wait() => match message {
                        // Both kinds of output reach the buffer. `ExtendedData` is the far
                        // end's standard error, and a session that rendered a command's
                        // output and silently dropped its error messages would be a session
                        // that lies to somebody who cannot see the screen.
                        Some(ChannelMsg::Data { data })
                        | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if bytes.send(data.to_vec()).await.is_err() {
                                break;
                            }
                        }
                        // The far end closed: the shell exited, or the connection went.
                        // Ending the loop drops the sender, which is how the domain above
                        // learns a session is over (the port's own rule).
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
                            // The byte first, because it is what a remote pty's line
                            // discipline turns into `SIGINT`; the request as well, for a
                            // server that acts on one. Neither is asserted to have worked:
                            // whether a command actually ended is observed the ordinary
                            // way, in the bytes that follow (the port's own rule).
                            let _ = write.data_bytes(vec![INTERRUPT]).await;
                            let _ = write.signal(Sig::INT).await;
                        }
                        Some(Outgoing::Resize { columns, screen_lines }) => {
                            let _ = write
                                .window_change(u32::from(columns), u32::from(screen_lines), 0, 0)
                                .await;
                        }
                        // Everything above has let go of this transport.
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

/// Signing in, with the password asked for only when the server will take one.
///
/// **The question is asked because the server offered the method, not because Acter
/// assumed it.** A server that takes only public keys is told so by the user's client, and
/// asking for a password there would be asking for a secret that could not be used —
/// exactly the reason the password is not a field on the connect form (spec B9, decision 4).
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
            .map_err(|why| {
                ended(format!(
                    "Acter could not sign in to {host} as {user}. {why}"
                ))
            })?;
        if result.success() {
            return Ok(());
        }
        // The server may stop offering passwords after enough refusals, and a dialog that
        // reappeared forever would be a dialog nobody can get out of except by guessing.
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

/// Whether the server is still willing to be given a password.
fn offers_password(result: &russh::client::AuthResult) -> bool {
    match result {
        russh::client::AuthResult::Success => false,
        russh::client::AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods.contains(&russh::MethodKind::Password),
    }
}

/// Asks a question on a thread of its own.
///
/// **Nothing that waits for a person may hold a runtime worker.** The port is sync because
/// ARCHITECTURE says port traits are, and answering means a dialog somebody has to read; a
/// blocking call straight from this task would park one of the runtime's threads for as
/// long as that takes. `spawn_blocking` is the thread pool that exists for exactly this,
/// and the port travels into it as an `Arc` rather than a borrow because the task outlives
/// this stack frame as far as the compiler is concerned.
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

/// The client handler: the one thing russh calls back into, and the place the security
/// decision is made.
///
/// `Clone` because the decision itself is made on a blocking task — everything in it is
/// shared or copied, so a clone decides exactly what the original would have.
#[derive(Clone)]
struct Verifier {
    hosts: Arc<KnownHosts>,
    questions: Arc<dyn SshQuestions>,
    host: String,
    port: u16,
    /// Why the connection was refused, when it was refused by a person rather than by the
    /// network. Read after `connect` fails, so the sentence a listener hears is about their
    /// own decision rather than about a socket.
    refusal: Arc<Mutex<Option<String>>>,
}

impl Verifier {
    /// Records the sentence to report instead of whatever the library says next.
    fn refuse(&self, why: String) {
        *self.refusal.lock().expect("refusal lock poisoned") = Some(why);
    }

    /// The whole of decision 3, in one place: a key that is already recorded connects
    /// silently, and anything else is a question whose default is refusal.
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
            // Not a failure to connect: the user accepted the key and the session should
            // happen. What they are owed is knowing they will be asked again.
            self.questions.tell(&why);
        }
        true
    }
}

impl client::Handler for Verifier {
    type Error = russh::Error;

    /// **Acter never silently trusts** (spec B9, decision 3). There is no accept-everything
    /// mode and no `StrictHostKeyChecking no` to find: this is the only path, and its
    /// default is to ask.
    async fn check_server_key(
        &mut self,
        offered: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match offered {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            // A host certificate is a real thing and Acter cannot yet judge one: doing it
            // properly means knowing which certificate authorities the user trusts, which
            // is `@cert-authority` in a file this entry does not parse. Refusing loudly is
            // the honest answer; asking a person to accept something Acter could not check
            // would be asking them to guess.
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
