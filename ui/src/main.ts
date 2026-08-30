// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
import { BeepAudio } from './adapters/beep';
import { BufferDom } from './adapters/buffer';
import { installDebugRecorder } from './adapters/debug_recorder';
import { AboutDialog } from './adapters/about_dialog';
import { EditFieldDom } from './adapters/edit_field';
import { HelpDialog } from './adapters/help_dialog';
import { bindKeys } from './adapters/keyboard';
import { ConnectDialog } from './adapters/connect_dialog';
import { HostKeyDialog } from './adapters/host_key_dialog';
import { MessageDialog } from './adapters/message_dialog';
import { PasswordDialog } from './adapters/password_dialog';
import { SetUpDialog } from './adapters/set_up_dialog';
import { UnverifiedDialog } from './adapters/unverified_dialog';
import { installMenuBar } from './adapters/menu_bar';
import { WindowChrome } from './adapters/window_chrome';
import { AppController } from './controllers/app';
import { TauriBackend, TauriConnect, TauriShell } from './routers/tauri';
import type { ConnectAnswer, ConnectQuestion } from './protocol';

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing element: ${id}`);
  }
  return element as T;
}

const commandInput = byId<HTMLInputElement>('command-input');
const editField = new EditFieldDom(commandInput);
const buffer = new BufferDom(byId('results'));
const announcer = new AnnouncerDom(byId('announcer'));
const beep = new BeepAudio();
// In a debug build this wraps the router and installs `window.__acterDebug`; in a
// release build it hands the router straight back and installs nothing.
// What the window says it is: the operating system's title, the document's heading, and
// the connection status, all from one adapter (spec A9).
const windowChrome = new WindowChrome({
  heading: byId('window-title'),
  statusRegion: byId('connection-status'),
  notConnectedWindow: byId('not-connected-window'),
  connectButton: byId('connect-button'),
  terminalWindow: byId('terminal-window'),
  results: byId('results'),
  buffer,
  form: byId('command-form'),
  editField,
  ended: byId('terminal-ended'),
  reconnectButton: byId('reconnect-button'),
  document,
  setNativeTitle: (title: string) => void shell.setTitle(title),
});
const connectApi = new TauriConnect();
// The two questions an SSH connection asks, each its own dialog (spec B9, decisions 3
// and 4). They are built before the controller because it is handed the thing that asks,
// and they know nothing about connecting — one shows a fingerprint and comes back with a
// decision, the other shows a masked field and comes back with a secret.
const hostKeyDialog = new HostKeyDialog(
  byId<HTMLDialogElement>('host-key-dialog'),
  byId('host-key-title'),
  byId('host-key-summary'),
  byId('host-key-body'),
);
const passwordDialog = new PasswordDialog(
  byId<HTMLDialogElement>('password-dialog'),
  byId('password-prompt'),
  byId<HTMLInputElement>('password-field'),
);
// The third question, and the one that is about this machine rather than the far end: the
// file about to be started did not verify (spec B5.7, decision 6). It is built beside the
// other two and knows nothing about connecting either — it shows a path and a signer and
// comes back with a decision.
const unverifiedDialog = new UnverifiedDialog(
  byId<HTMLDialogElement>('unverified-dialog'),
  byId('unverified-summary'),
  byId('unverified-body'),
);
// The fourth question, and the one that is not a warning: the connection has succeeded, the
// far end has said what shell it runs, and this is the command Acter would run inside the
// session (spec B9.5, decision 9). Built beside the other three and knowing nothing about
// connecting either — it shows a command and comes back with a decision.
const setUpDialog = new SetUpDialog(
  byId<HTMLDialogElement>('set-up-dialog'),
  byId('set-up-summary'),
  byId('set-up-body'),
  byId<HTMLInputElement>('set-up-remember'),
);
// Which dialog a question goes to is decided by the question's own shape, so a variant
// added to the protocol without a dialog to put it in fails to compile rather than
// silently reaching nobody.
const questions = {
  ask: (question: ConnectQuestion): Promise<ConnectAnswer> => {
    switch (question.question) {
      case 'HostKey':
        return hostKeyDialog.ask(question);
      case 'Password':
        return passwordDialog.ask(question);
      case 'Unverified':
        return unverifiedDialog.ask(question);
      case 'SetUpSession':
        return setUpDialog.ask(question);
    }
  },
};
// A connection that failed is acknowledged rather than announced (reported 2026-08-26).
const failureDialog = new MessageDialog(
  byId<HTMLDialogElement>('failed-dialog'),
  byId('failed-why'),
);
const controller = new AppController(
  installDebugRecorder(new TauriBackend()),
  connectApi,
  editField,
  buffer,
  announcer,
  beep,
  windowChrome,
  questions,
  failureDialog,
);

// The menu bar is in the document rather than in the window frame, and F10 is the way
// into it (spec A7). Its two items are handed the things they act on, so the bar itself
// knows about neither the shell nor the dialog.
const shell = new TauriShell();
// Connecting is three named backend actions and this is the thinnest caller of them: the
// dialog renders what `connectable()` answered and hands a chosen profile back to the
// controller, which owns the buffer, the titles and the words (spec A8).
const connectDialog = new ConnectDialog(
  byId<HTMLDialogElement>('connect-dialog'),
  byId('connect-kinds'),
  byId('connect-panel-title'),
  byId('connect-panel-body'),
  connectApi,
  (id, setUp) => controller.connectTo(id, setUp),
  announcer,
  windowChrome,
);
// One handler, both buttons: the two windows are exclusive, so a listener never meets both,
// and the action they run is the same one the menu item runs (spec A10).
for (const id of ['connect-button', 'reconnect-button']) {
  byId(id).addEventListener('click', () => void connectDialog.open());
}
// What a session can and cannot tell you, explained where a listener can read it at
// their own pace rather than in an announcement that is heard once (spec A13, decision 2).
// It is built outside the Windows-only block below, because F1 opens it on every platform
// while the menu bar exists only where a native one would freeze the reader.
const helpDialog = new HelpDialog(
  byId<HTMLDialogElement>('help-dialog'),
  windowChrome,
);
const aboutDialog = new AboutDialog(
  byId<HTMLDialogElement>('about-dialog'),
  shell,
  windowChrome,
);
// Windows only, and asked rather than assumed: a native menu bar is the right answer on
// macOS, where menus live in the system bar, and this one exists because Windows is the
// platform where a native menu freezes the screen reader (spec A7). Elsewhere the region
// is removed outright rather than left hidden, so nothing empty is in the document.
const menuBarRegion = byId('menu-bar-region');
void shell.platform().then((os) => {
  if (os !== 'windows') {
    menuBarRegion.remove();
    return;
  }
  installMenuBar(
    byId('menu-bar'),
    {
      connect: () => void connectDialog.open(),
      exit: () => void shell.exit(),
      help: () => helpDialog.open(),
      about: () => void aboutDialog.open(),
    },
    // Where the menu returns to is "whatever this window is showing" rather than the edit
    // field by name: since A10 there is not always one, and focusing a hidden input does
    // nothing at all — which left a listener stranded on the menu item they had just closed
    // (measured with NVDA 2026-08-26).
    windowChrome,
  );
  // **Revealed after it is wired, never before.** A menu bar in the accessibility tree that
  // does not answer F10 yet is a menu bar that is not there, and a listener who presses for
  // it in that window hears nothing and has no way to tell why. The two lines used to be the
  // other way round, which also made `menu.spec.ts`'s guard watch a proxy for "the listeners
  // are attached" rather than the thing itself.
  menuBarRegion.hidden = false;
});

// The edit field is passed because the session hears a keystroke only while that field
// has focus (DESIGN, layer 2), and the adapter enforces that by listening on the element
// rather than on the document.
bindKeys(controller, byId<HTMLFormElement>('command-form'), commandInput, () =>
  helpDialog.open(),
);
// What this window opens onto: the session the launch brought, or nothing at all — which
// since B7 is the ordinary case, and which the controller announces rather than leaving a
// listener in front of a window that says nothing (spec B7, decision 3). Naming the far end
// is part of the same call now: a session can be replaced while the window is open, so the
// title comes from the connection rather than from what the process was started with.
// **Focus is the controller's now**, because where it belongs depends on which of the two
// faces the window opens with: the edit field when a launch brought a session, the Connect
// button when it did not (spec A10). `WindowChrome.showTerminal` places it as it shows.
void controller.start();
