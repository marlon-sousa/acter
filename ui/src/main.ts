// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
import { BeepAudio } from './adapters/beep';
import { BufferDom } from './adapters/buffer';
import { installDebugRecorder } from './adapters/debug_recorder';
import { AboutDialog } from './adapters/about_dialog';
import { EditFieldDom } from './adapters/edit_field';
import { FarEndFieldDom } from './adapters/far_end_field';
import { HelpDialog } from './adapters/help_dialog';
import { bindKeys } from './adapters/keyboard';
import { ConnectDialog } from './adapters/connect_dialog';
import { ConnectingDialog } from './adapters/connecting_dialog';
import { NewConnectionDialog } from './adapters/new_connection_dialog';
import {
  ForgetConnectionDialog,
  RenameConnectionDialog,
} from './adapters/rename_connection_dialog';
import { SaveConnectionDialog, suggestion } from './adapters/save_connection_dialog';
import { HostKeyDialog } from './adapters/host_key_dialog';
import { MessageDialog } from './adapters/message_dialog';
import { PasswordDialog } from './adapters/password_dialog';
import { SetUpDialog } from './adapters/set_up_dialog';
import { UnverifiedDialog } from './adapters/unverified_dialog';
import { installMenuBar } from './adapters/menu_bar';
import { applyPlatformText } from './adapters/platform_text';
import { installSystemMenu } from './adapters/system_menu';
import { WindowChrome } from './adapters/window_chrome';
import { AppController, nothingToSaveMessage } from './controllers/app';
import {
  TauriBackend,
  TauriConnect,
  TauriShell,
  TauriSystemMenu,
} from './routers/tauri';
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
// The far end's command line: an ARIA text box Acter writes into and never inserts into
// (spec 28, decision 2). Hidden until the user hands the keyboard over with Ctrl+Shift+K.
const farEndInput = byId('far-end-input');
const farEndField = new FarEndFieldDom(farEndInput, byId('far-end-line'));
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
  form: byId('command-form'),
  editField,
  farEndField,
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
  farEndField,
  announcer,
  beep,
  windowChrome,
  questions,
  failureDialog,
);

// What a session can and cannot tell you, explained where a listener can read it at their
// own pace rather than in an announcement that is heard once (spec A13, decision 2).
//
// It is built here, before the Connect dialog, because that dialog's Help button opens it
// at the section about the checkbox it sits beside (reported 2026-08-30). F1 opens it on
// every platform, which is why it is outside the Windows-only block below.
const helpDialog = new HelpDialog(
  byId<HTMLDialogElement>('help-dialog'),
  windowChrome,
);
// The menu bar is in the document rather than in the window frame, and F10 is the way
// into it (spec A7). Its two items are handed the things they act on, so the bar itself
// knows about neither the shell nor the dialog.
const shell = new TauriShell();
// Where Enter goes while a connection is being made: a dialog that names the far end and
// carries the backend's progress sentences, instead of the list of kinds this used to put
// a listener back on (reported 2026-08-30).
const connectingDialog = new ConnectingDialog(
  byId<HTMLDialogElement>('connecting-dialog'),
  byId('connecting-what'),
);
// **New connection: A8's dialog, renamed** (spec 26, decision 17). Connecting is a set of
// named backend actions and this is the thinnest caller of them: the dialog renders what
// `connectable()` answered and hands a chosen profile back to the controller, which owns
// the buffer, the titles and the words (spec A8).
const newConnectionDialog = new NewConnectionDialog(
  byId<HTMLDialogElement>('new-connection-dialog'),
  byId('new-kinds'),
  byId('new-panel-title'),
  byId('new-panel-body'),
  connectApi,
  (id, setUp, origin) => controller.connectTo(id, setUp, origin),
  announcer,
  windowChrome,
  connectingDialog,
  helpDialog,
  () => void afterConnecting(),
);
// The three dialogs that act on one saved connection each, built before the dialog that
// opens them: it takes them rather than reaching for them, which is the rule every adapter
// here is written under.
const saveConnectionDialog = new SaveConnectionDialog(
  byId<HTMLDialogElement>('save-connection-dialog'),
  byId('save-why'),
  byId<HTMLInputElement>('save-name'),
  byId('save-also-later'),
  byId<HTMLInputElement>('save-not-again'),
  byId<HTMLButtonElement>('save-ok'),
  byId<HTMLButtonElement>('save-cancel'),
  windowChrome,
);
const renameConnectionDialog = new RenameConnectionDialog(
  byId<HTMLDialogElement>('rename-connection-dialog'),
  byId<HTMLInputElement>('rename-name'),
  byId<HTMLButtonElement>('rename-ok'),
  byId<HTMLButtonElement>('rename-cancel'),
);
const forgetConnectionDialog = new ForgetConnectionDialog(
  byId<HTMLDialogElement>('forget-connection-dialog'),
  byId('forget-why'),
  byId<HTMLButtonElement>('forget-ok'),
  byId<HTMLButtonElement>('forget-cancel'),
);
// **Connect: the list of names somebody saved** (spec 26, decisions 12 to 16).
const connectDialog = new ConnectDialog(
  byId<HTMLDialogElement>('connect-dialog'),
  byId('connect-names'),
  byId('connect-empty'),
  byId('connect-panel-title'),
  byId('connect-panel-body'),
  connectApi,
  (id, setUp, origin) => controller.connectTo(id, setUp, origin),
  announcer,
  windowChrome,
  connectingDialog,
  {
    rename: (name) => renameConnectionDialog.ask(name),
    forget: (question) => forgetConnectionDialog.ask(question),
  },
  () => void newConnectionDialog.open(),
  () => void controller.announceConnection(),
);
// One handler, both buttons: the two windows are exclusive, so a listener never meets both,
// and the action they run is the same one the menu item runs (spec A10).
for (const id of ['connect-button', 'reconnect-button']) {
  byId(id).addEventListener('click', () => void connectDialog.open());
}

/**
 * The three sentences a listener hears after a new connection, in order (spec 26,
 * decision 19): the connection, who has the keys, and — when a save happened — the receipt.
 *
 * **The offer comes after the first two**, because the connection is the news, the keys are
 * what the next keypress needs, and the save is a receipt. It is made only once per
 * session and only for one nobody has named.
 */
async function afterConnecting(): Promise<void> {
  controller.announceConnection();
  await controller.offerToSave(async (connected) => {
    const answer = await saveConnectionDialog.ask(suggestion(connected), true);
    if (answer.name === null) {
      return answer;
    }
    await saveUnder(answer.name, saveConnectionDialog);
    return answer;
  });
}

/**
 * Save under this name, keeping the dialog open while the backend refuses.
 *
 * **A refusal keeps the dialog open with the sentence announced and focus back in the
 * field** (decision 18), which is where trying something else begins. The words are the
 * backend's, because a name can arrive from somewhere that never saw a dialog.
 */
async function saveUnder(name: string, dialog: SaveConnectionDialog): Promise<void> {
  let asking = name;
  for (;;) {
    const said = await controller.saveConnection(asking);
    if (said !== null) {
      dialog.finish();
      // The receipt, after the connection sentence and the keys sentence.
      announcer.announce(said);
      return;
    }
    dialog.refused();
    const again = await dialog.ask(asking, false);
    if (again.name === null) {
      return;
    }
    asking = again.name;
  }
}

/** File → Save connection, from the menu, at any time (decision 18). */
async function saveConnection(): Promise<void> {
  const connected = controller.connectedNow;
  if (connected === null) {
    // **No dialog at all**: there is nothing to name, and a dialog whose only honest
    // content is a refusal is a dialog nobody should have to escape from.
    announcer.announce(nothingToSaveMessage);
    return;
  }
  const answer = await saveConnectionDialog.ask(suggestion(connected), false);
  if (answer.name === null) {
    return;
  }
  await saveUnder(answer.name, saveConnectionDialog);
}
const aboutDialog = new AboutDialog(
  byId<HTMLDialogElement>('about-dialog'),
  shell,
  windowChrome,
);
// What the menu items do, and the one list of them: the document menu bar on Windows and
// the operating system's own menu bar on macOS are two ways into the same four actions, so
// neither platform can drift from the other by an edit to one of them (spec M3, decision 5).
const menuActions = {
  connect: () => void connectDialog.open(),
  newConnection: () => void newConnectionDialog.open(),
  saveConnection: () => void saveConnection(),
  exit: () => void shell.exit(),
  help: () => helpDialog.open(),
  about: () => void aboutDialog.open(),
};
// Windows only, and asked rather than assumed: a native menu bar is the right answer on
// macOS, where menus live in the system bar, and this one exists because Windows is the
// platform where a native menu freezes the screen reader (spec A7). Elsewhere the region is
// removed outright rather than left hidden, so nothing empty is in the document — which
// since M3 is `data-platform`'s doing rather than a line of its own, because the help now
// has sentences that belong to one platform for exactly the same reason.
const menuBarRegion = byId('menu-bar-region');
void shell.platform().then((os) => {
  applyPlatformText(document, os);
  if (os !== 'windows') {
    // The menu Acter did not draw. macOS builds it natively from what the backend decided,
    // and what reaches the window is only the items that open a dialog; a platform with no
    // native menu emits nothing, so this subscribes and never hears anything (spec M3).
    installSystemMenu(new TauriSystemMenu(), menuActions, windowChrome);
    return;
  }
  installMenuBar(
    byId('menu-bar'),
    menuActions,
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
bindKeys(
  controller,
  byId<HTMLFormElement>('command-form'),
  commandInput,
  () => helpDialog.open(),
  farEndInput,
);
// What this window opens onto: the session the launch brought, or nothing at all — which
// since B7 is the ordinary case, and which the controller announces rather than leaving a
// listener in front of a window that says nothing (spec B7, decision 3). Naming the far end
// is part of the same call now: a session can be replaced while the window is open, so the
// title comes from the connection rather than from what the process was started with.
// **Focus is the controller's now**, because where it belongs depends on which of the two
// faces the window opens with: the edit field when a launch brought a session, the Connect
// button when it did not (spec A10). `WindowChrome.showTerminal` places it as it shows.
//
// **And then the launch switch, carried out here rather than behind the window's back**
// (spec 26, decision 20). `acter --connect <name>` becomes a request the backend answers
// and this acts on, through the same call the Connect dialog makes — so a saved SSH
// connection asks its host-key and password questions in front of the person who can
// answer them, which is what B9 already requires of every SSH attempt. A name nothing is
// saved under leaves the window unconnected and says so.
void controller.start().then(async () => {
  if (await controller.carryOutTheLaunchSwitch()) {
    // The same three sentences a connection made from a dialog gets, in the same order —
    // except that a saved connection already has a name, so nothing is offered.
    await afterConnecting();
  }
});
