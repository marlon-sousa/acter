//! Adapter: [`LocalPty`] — a real shell on a local pseudoconsole, behind acter-core's
//! [`Transport`] port.
//!
//! The second implementer of a seam that has worked since B3.5, not a new one: everything
//! above it — the engine, the tracker, the correlation, the pacing policy — has been
//! running against a scripted far end and does not change because these bytes came from
//! ConPTY instead. What is new is that nobody wrote them down in advance.
//!
//! **It names no shell.** The program to spawn is the caller's, and so is the environment
//! it is spawned with, because which shell to run and what to inject into it is
//! `ShellAdapter`'s knowledge and therefore B5's —
//! DESIGN's transport-versus-shell criterion, the same one that put [`Transport::interrupt`]
//! on this port and left EOF off it. A transport reaching for `powershell.exe` would be
//! deciding a shell question at the transport seam, and the SSH adapter beside it would
//! then have to decide the same question differently.
//!
//! **Reading is a thread, and one read is one send.** The port's own doc chose this: a
//! blocking pseudoconsole read gets a dedicated thread feeding the session channel, which
//! is exactly the reading strategy that differs between implementers. The thread sends
//! precisely what each read returned and never merges two of them to make the stream
//! tidier — chunk boundaries are what DESIGN's reliability cases are about, and this is
//! where they stop being simulated.
//!
//! **It clears the inherited "ignore Ctrl+C" attribute before it spawns.** See
//! [`allow_ctrl_c_below_us`]: the one line that makes an interrupt mean something, and
//! the reason it lives here rather than beside [`Transport::interrupt`].
//!
//! **Failure to start is not this type's to swallow.** [`LocalPty::spawn`] does the
//! opening and the spawning and reports a speakable sentence if either fails, the way
//! `SessionTranscript::load` does: a session that could not start must be a loud failure
//! at the composition root rather than a window that opens onto silence.

use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use acter_core::{Transport, TransportError};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::sync::mpsc::Sender;

/// How much one read may return. A pseudoconsole hands over whatever it has, so this is
/// a ceiling rather than a target: a small write arrives as a small read, which is the
/// property the domain above is entitled to see.
const READ_BUFFER: usize = 8 * 1024;

/// End of text — what a console turns into an interrupt for the attached process. The
/// reason [`Transport::interrupt`] is a port method and not bytes a service computes:
/// here it travels *in* the data stream, and over SSH the same intent is a channel
/// request that travels outside it.
const INTERRUPT: u8 = 0x03;

/// One session on a local pseudoconsole.
pub struct LocalPty {
    /// The pseudoconsole itself, shared with the thread waiting on the shell — which
    /// takes it when the shell exits.
    ///
    /// **Dropping it is how a session ends**, and that is not a detail: a pseudoconsole
    /// stays open, and its reader stays blocked, for as long as anything holds the
    /// master — the child exiting does not close it. So a shell that ended would leave
    /// the read channel open forever, and the domain above would go on accepting
    /// commands into a session with nothing at the far end, saying nothing about it,
    /// which is the worst shape a failure can take for a user who cannot see the window.
    /// Found by the real-shell test that asserts the session ends (spec B4, decision 3).
    master: Shared,
    /// A handle that can kill the shell, kept because the shell itself is owned by the
    /// waiting thread. A shell that outlives the window that opened it is a leak the
    /// user cannot see.
    killer: Box<dyn ChildKiller + Send + Sync>,
    wire: Wire,
    /// Taken by [`Transport::start`], which is what makes starting twice a no-op rather
    /// than two threads racing over one pseudoconsole.
    reader: Option<Box<dyn Read + Send>>,
}

/// The pseudoconsole, shared between this adapter and the thread that outlives the shell.
/// `None` once the shell has exited and the pseudoconsole has been closed.
type Shared = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

impl LocalPty {
    /// Opens a pseudoconsole of this size and spawns `program` on it.
    ///
    /// The error is a whole spoken sentence, not a fragment: it reaches the user as the
    /// answer to "why is there no session", and by CLAUDE.md that is a domain requirement
    /// rather than polish.
    /// The environment is the caller's for the same reason the program is: what to inject
    /// into a shell is `ShellAdapter`'s knowledge, and `cmd.exe`'s whole OSC 133 injection
    /// is one variable (spec B4.5, decision 1). A transport reaching for `PROMPT` itself
    /// would be deciding a shell question at the transport seam.
    ///
    /// Variables are added to the ones this process already has rather than replacing
    /// them: a shell started with an empty environment is not the shell the user asked for.
    pub fn spawn(
        program: &str,
        args: &[&str],
        environment: &[(&str, &str)],
        columns: u16,
        screen_lines: u16,
    ) -> Result<Self, String> {
        let pty = native_pty_system()
            .openpty(size(columns, screen_lines))
            .map_err(|why| {
                format!("Acter could not open a terminal for the shell to run in. {why}")
            })?;

        allow_ctrl_c_below_us();

        let mut command = CommandBuilder::new(program);
        command.args(args);
        for (name, value) in environment {
            command.env(name, value);
        }
        let child = pty
            .slave
            .spawn_command(command)
            .map_err(|why| format!("Acter could not start the shell {program}. {why}"))?;
        let killer = child.clone_killer();

        // Both are taken now rather than lazily: a failure here is a session that will
        // never work, and it must be reported beside the spawn rather than surfacing
        // later as a write that mysteriously cannot be delivered.
        let reader = pty.master.try_clone_reader().map_err(|why| {
            format!("Acter started the shell {program} but cannot read from it. {why}")
        })?;
        let writer = pty.master.take_writer().map_err(|why| {
            format!("Acter started the shell {program} but cannot write to it. {why}")
        })?;

        // The slave goes out of scope here, which matters on Windows: the shell has its
        // end of the pseudoconsole now, and anything still holding this one would keep
        // the session's pipes open past the shell's own life.
        drop(pty.slave);

        let master: Shared = Arc::new(Mutex::new(Some(pty.master)));
        watch(child_waiter(child), Arc::clone(&master));

        Ok(Self {
            master,
            killer,
            wire: Wire::new(writer),
            reader: Some(reader),
        })
    }
}

/// Restores normal Ctrl+C processing for this process, so nothing spawned below it
/// inherits a refusal to be interrupted (spec B4.1, decision 1).
///
/// **This is what makes [`Transport::interrupt`] mean anything**, and it is a property of
/// the *spawn* rather than of the byte written afterwards. `SetConsoleCtrlHandler(NULL,
/// TRUE)` sets a process attribute that children inherit, and `CREATE_NEW_PROCESS_GROUP`
/// sets it implicitly for a whole group — which is how launchers routinely spawn children
/// so they can kill trees. A shell spawned while it is set ignores Ctrl+C however
/// correctly the interrupt is encoded, because the target is refusing the signal rather
/// than never receiving it. Measured against a real `cmd.exe` running `ping -n 20`: four
/// further replies after the interrupt with the attribute inherited, none at all after
/// this call.
///
/// **The inheritance is transitive, and that is the whole mechanism.** Acter spawns
/// exactly one process and holds one child: everything the shell launches is on the far
/// side of the pseudoconsole, with no pid and no handle on this side. The program that
/// has to receive the interrupt is not the shell but whatever the shell ran, and this
/// adapter can never touch it — it does not need to, because the attribute passes from
/// Acter to shell to program on its own. So the fix is not "make the shell accept
/// Ctrl+C", it is "do not poison the state that everything below us inherits", and our
/// own process before the spawn is the only place to do it. Acter enumerates and
/// terminates nothing: that would be doing the kernel's job from the wrong layer, and it
/// would cost what a real Ctrl+C preserves — handlers running, cleanup happening, the
/// program choosing its own exit code.
///
/// The return value is deliberately dropped. A failure here means interrupts will not
/// take effect, which is not a reason to refuse to open a session, and there is nothing
/// truthful to say about it at this point: what the user will observe is a prompt that
/// does not come back, which the interrupt path already reports by saying nothing.
///
/// Nothing to do off Windows: a bare `0x03` on a Unix pty is turned into `SIGINT` by the
/// line discipline, with no process attribute in the way.
fn allow_ctrl_c_below_us() {
    #[cfg(windows)]
    // SAFETY: a Win32 call taking no pointer we own. Passing a null routine with `FALSE`
    // asks for the default handler to be restored, which is documented and has no
    // lifetime attached to it.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0);
    }
}

/// Waits for the shell to exit and then closes the pseudoconsole, so the reading thread's
/// blocking read returns and the session ends by the channel closing — the only ending
/// the `Transport` port has.
///
/// A thread rather than a task: `wait` blocks, and this crate's rule is that blocking
/// belongs on threads of its own (the same reason the reader is one).
fn watch<T: Send + 'static>(mut wait: impl FnMut() + Send + 'static, held: Arc<Mutex<Option<T>>>) {
    thread::spawn(move || {
        wait();
        // Taking it drops it, which closes the pseudoconsole. A poisoned lock would mean
        // a panic while the master was in hand, which nothing here does; recovering it is
        // still better than leaving a session that cannot end.
        let _ = held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    });
}

/// Boxes the wait so [`watch`] is testable without a process: what it does *after* the
/// wait is the part worth pinning.
fn child_waiter(mut child: Box<dyn portable_pty::Child + Send + Sync>) -> impl FnMut() + Send {
    move || {
        let _ = child.wait();
    }
}

impl Transport for LocalPty {
    /// Starts the reading thread. Every read it returns becomes exactly one send.
    ///
    /// `blocking_send` because this thread is not async and must not become so: it is
    /// parked in a blocking read almost all of its life. The back-pressure that gives is
    /// honest — a far end producing faster than the domain can absorb is slowed down
    /// rather than queued without limit, which is the same answer `SessionService`'s
    /// bounded read channel already made.
    ///
    /// The session ends by this thread returning: the sender is dropped, the domain's
    /// read channel closes, and the pump stops. A shell that exited is not an error and
    /// has no error path here.
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let Some(mut reader) = self.reader.take() else {
            return;
        };
        self.wire.start();
        thread::spawn(move || {
            let mut buffer = vec![0u8; READ_BUFFER];
            loop {
                match reader.read(&mut buffer) {
                    // Zero is the far end closing; an error on a pseudoconsole read is
                    // the same event wearing a different coat (Windows reports a closed
                    // console as a broken pipe), and neither is worth an event of its own.
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if bytes.blocking_send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.wire.write(bytes)
    }

    fn interrupt(&mut self) -> Result<(), TransportError> {
        self.wire.write(&[INTERRUPT])
    }

    /// Resizing a session whose shell has gone answers that the session ended, rather
    /// than reporting success against a pseudoconsole that no longer exists.
    fn resize(&mut self, columns: u16, screen_lines: u16) -> Result<(), TransportError> {
        let master = self
            .master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(master) = master.as_ref() else {
            return Err(TransportError::Closed);
        };
        master
            .resize(size(columns, screen_lines))
            // The same sentence treatment a failed write gets: this reaches a listener
            // as the answer to "what happened", not as a log line.
            .map_err(|why| TransportError::Failed {
                detail: sentence(&why.to_string()),
            })
    }
}

impl Drop for LocalPty {
    /// The shell goes when the session goes. Without this a torn-down session leaves a
    /// live shell attached to a pseudoconsole nobody is reading, which the user has no
    /// way to notice and no way to reach.
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

/// The writing half of a started session, separated from the pseudoconsole it belongs to
/// so that what this adapter *decides* — which failure is the end of the session and
/// which is a failed write — can be tested without spawning anything.
struct Wire {
    writer: Box<dyn Write + Send>,
    /// Whether the session has been started. The port's contract, kept rather than
    /// quietly improved on: this adapter *could* write before `start`, because the shell
    /// is spawned by the constructor, and it refuses anyway so that every implementer
    /// answers the same way.
    started: bool,
}

impl Wire {
    fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer,
            started: false,
        }
    }

    fn start(&mut self) {
        self.started = true;
    }

    /// Writes and flushes, because a shell waiting on a line the operating system is
    /// still holding looks exactly like a shell that has gone quiet.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        if !self.started {
            return Err(TransportError::NotStarted);
        }
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|why| match why.kind() {
                // The far end is gone. Not a failure of this write in particular — the
                // read thread is about to say the same thing properly by closing its
                // channel — so it gets the variant whose sentence says the session ended.
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::NotConnected => {
                    TransportError::Closed
                }
                _ => TransportError::Failed {
                    detail: sentence(&why.to_string()),
                },
            })
    }
}

fn size(columns: u16, screen_lines: u16) -> PtySize {
    PtySize {
        rows: screen_lines.max(1),
        cols: columns.max(1),
        // The pixel dimensions matter to programs that draw with sixels and to nothing
        // this product does yet.
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// The world's words, made into a sentence a screen reader can end on.
///
/// An operating-system error arrives as a lowercase fragment with no full stop, and
/// `TransportError::Failed` appends it as its own sentence — so it has to be one.
fn sentence(detail: &str) -> String {
    let mut characters = detail.trim().chars();
    let sentence: String = match characters.next() {
        None => return "The operating system gave no reason.".to_owned(),
        Some(first) => first.to_uppercase().chain(characters).collect(),
    };
    if sentence.ends_with(['.', '!', '?']) {
        sentence
    } else {
        format!("{sentence}.")
    }
}

#[cfg(test)]
mod tests {
    use std::io::Error;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// A writer that records, or fails with a chosen error kind.
    #[derive(Clone, Default)]
    struct FakeWriter {
        written: Arc<Mutex<Vec<u8>>>,
        fails: Option<ErrorKind>,
    }

    impl Write for FakeWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(kind) = self.fails {
                return Err(Error::new(kind, "the handle is invalid"));
            }
            self.written.lock().expect("writer poisoned").extend(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn unstarted(fails: Option<ErrorKind>) -> (Wire, Arc<Mutex<Vec<u8>>>) {
        let writer = FakeWriter {
            fails,
            ..FakeWriter::default()
        };
        let written = Arc::clone(&writer.written);
        (Wire::new(Box::new(writer)), written)
    }

    /// A wire the session has started, which is every test below except the last.
    fn wire(fails: Option<ErrorKind>) -> (Wire, Arc<Mutex<Vec<u8>>>) {
        let (mut wire, written) = unstarted(fails);
        wire.start();
        (wire, written)
    }

    #[test]
    fn what_is_written_reaches_the_far_end_unchanged() {
        let (mut wire, written) = wire(None);

        wire.write(b"git status\n").expect("the write succeeds");

        assert_eq!(&*written.lock().expect("writer poisoned"), b"git status\n");
    }

    /// The whole of an interrupt over a local pseudoconsole: one control byte in the data
    /// stream. Pinned because the byte is the mechanism — a different one would be a
    /// character the shell types out rather than an interrupt it acts on.
    #[test]
    fn an_interrupt_is_the_single_byte_0x03() {
        let (mut wire, written) = wire(None);

        wire.write(&[INTERRUPT]).expect("the write succeeds");

        assert_eq!(&*written.lock().expect("writer poisoned"), &[0x03]);
    }

    /// A broken pipe is the session ending, not this write failing. The read thread is
    /// about to report the same thing by closing its channel, so what the user hears is
    /// "the session has ended" rather than an operating-system fragment.
    #[test]
    fn a_far_end_that_went_away_is_reported_as_the_session_ending() {
        for kind in [
            ErrorKind::BrokenPipe,
            ErrorKind::ConnectionReset,
            ErrorKind::NotConnected,
        ] {
            let (mut wire, _) = wire(Some(kind));

            assert_eq!(
                wire.write(b"anything"),
                Err(TransportError::Closed),
                "{kind}"
            );
        }
    }

    #[test]
    fn any_other_failure_carries_the_world_s_own_words() {
        let (mut wire, _) = wire(Some(ErrorKind::PermissionDenied));

        let Err(TransportError::Failed { detail }) = wire.write(b"anything") else {
            panic!("a permission failure is not the session ending");
        };
        assert_eq!(detail, "The handle is invalid.");
    }

    /// `TransportError::Failed` reads its detail out as a sentence of its own, so a
    /// lowercase fragment from the operating system has to be made into one.
    #[test]
    fn an_operating_system_fragment_becomes_a_speakable_sentence() {
        assert_eq!(sentence("the handle is invalid"), "The handle is invalid.");
        assert_eq!(sentence("Access is denied."), "Access is denied.");
        assert_eq!(sentence("   "), "The operating system gave no reason.");
    }

    /// The session ending, without a process to end: once the wait returns, whatever the
    /// session was holding is dropped.
    ///
    /// For a real session that thing is the pseudoconsole, and dropping it is what makes
    /// the reader's blocking read return. A shell exiting does not close a pseudoconsole
    /// by itself — which is the defect the real-shell suite found, and the reason this
    /// mechanism exists at all.
    #[test]
    fn what_the_session_holds_is_dropped_once_the_shell_has_been_waited_for() {
        let held = Arc::new(Mutex::new(Some("the pseudoconsole".to_owned())));
        let (exits, wait) = mpsc::channel::<()>();

        watch(
            move || {
                let _ = wait.recv();
            },
            Arc::clone(&held),
        );

        assert!(
            held.lock().expect("poisoned").is_some(),
            "still open while the shell is still running"
        );

        // The shell exits: the sender goes, so the wait returns.
        drop(exits);

        let mut closed = false;
        for _ in 0..500 {
            if held.lock().expect("poisoned").is_none() {
                closed = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            closed,
            "the pseudoconsole is closed once the shell has gone"
        );
    }

    /// The port's contract: a write before `start` is a caller ordering bug, and it is
    /// said out loud rather than swallowed — the same answer `ScriptedTransport` gives.
    #[test]
    fn writing_before_the_session_started_is_refused() {
        let (mut wire, _) = unstarted(None);

        assert_eq!(wire.write(b"anything"), Err(TransportError::NotStarted));
        assert_eq!(wire.write(&[INTERRUPT]), Err(TransportError::NotStarted));
    }
}
