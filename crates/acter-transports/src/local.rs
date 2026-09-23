//! Adapter: [`LocalPty`] — a real shell on a local pseudoconsole, behind acter-core's
//! [`Transport`] port.

use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use acter_core::{Transport, TransportError};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::sync::mpsc::Sender;

const READ_BUFFER: usize = 8 * 1024;

const INTERRUPT: u8 = 0x03;

pub struct LocalPty {
    /// Dropping this is what ends the session: the pseudoconsole and its reader stay open
    /// while anything holds the master, even after the shell exits.
    master: Shared,
    killer: Box<dyn ChildKiller + Send + Sync>,
    wire: Wire,
    reader: Option<Box<dyn Read + Send>>,
}

/// `None` once the shell has exited and the pseudoconsole has been closed.
type Shared = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

impl LocalPty {
    /// Opens a pseudoconsole of this size and spawns `program` on it.
    /// The error is a whole spoken sentence. `environment` is added to this process's own
    /// variables, not a replacement for them.
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

        let reader = pty.master.try_clone_reader().map_err(|why| {
            format!("Acter started the shell {program} but cannot read from it. {why}")
        })?;
        let writer = pty.master.take_writer().map_err(|why| {
            format!("Acter started the shell {program} but cannot write to it. {why}")
        })?;

        // Anything still holding the slave keeps the session's pipes open past the shell's life.
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

/// Clears the inherited "ignore Ctrl+C" process attribute, which must happen before the spawn
/// or the shell and everything it runs ignore [`Transport::interrupt`]; the measurement is in
/// docs/specs/b4.1-interrupt-that-interrupts.md.
fn allow_ctrl_c_below_us() {
    #[cfg(windows)]
    // SAFETY: takes no pointer; a null routine with FALSE restores the default handler.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0);
    }
}

fn watch<T: Send + 'static>(mut wait: impl FnMut() + Send + 'static, held: Arc<Mutex<Option<T>>>) {
    thread::spawn(move || {
        wait();
        let _ = held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    });
}

fn child_waiter(mut child: Box<dyn portable_pty::Child + Send + Sync>) -> impl FnMut() + Send {
    move || {
        let _ = child.wait();
    }
}

impl Transport for LocalPty {
    fn start(&mut self, bytes: Sender<Vec<u8>>) {
        let Some(mut reader) = self.reader.take() else {
            return;
        };
        self.wire.start();
        thread::spawn(move || {
            let mut buffer = vec![0u8; READ_BUFFER];
            loop {
                match reader.read(&mut buffer) {
                    // Windows reports a closed pseudoconsole as a broken pipe, not a zero read.
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
            .map_err(|why| TransportError::Failed {
                detail: sentence(&why.to_string()),
            })
    }
}

impl Drop for LocalPty {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

struct Wire {
    writer: Box<dyn Write + Send>,
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

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        if !self.started {
            return Err(TransportError::NotStarted);
        }
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|why| match why.kind() {
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
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// `TransportError::Failed` speaks its detail as a sentence of its own, so it must be one.
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

    #[test]
    fn an_interrupt_is_the_single_byte_0x03() {
        let (mut wire, written) = wire(None);

        wire.write(&[INTERRUPT]).expect("the write succeeds");

        assert_eq!(&*written.lock().expect("writer poisoned"), &[0x03]);
    }

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

    #[test]
    fn an_operating_system_fragment_becomes_a_speakable_sentence() {
        assert_eq!(sentence("the handle is invalid"), "The handle is invalid.");
        assert_eq!(sentence("Access is denied."), "Access is denied.");
        assert_eq!(sentence("   "), "The operating system gave no reason.");
    }

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

    #[test]
    fn writing_before_the_session_started_is_refused() {
        let (mut wire, _) = unstarted(None);

        assert_eq!(wire.write(b"anything"), Err(TransportError::NotStarted));
        assert_eq!(wire.write(&[INTERRUPT]), Err(TransportError::NotStarted));
    }
}
