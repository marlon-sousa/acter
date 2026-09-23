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
const farEndInput = byId('far-end-input');
const farEndField = new FarEndFieldDom(farEndInput, byId('far-end-line'));
const announcer = new AnnouncerDom(byId('announcer'));
const beep = new BeepAudio();
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
const unverifiedDialog = new UnverifiedDialog(
  byId<HTMLDialogElement>('unverified-dialog'),
  byId('unverified-summary'),
  byId('unverified-body'),
);
const setUpDialog = new SetUpDialog(
  byId<HTMLDialogElement>('set-up-dialog'),
  byId('set-up-summary'),
  byId('set-up-body'),
  byId<HTMLInputElement>('set-up-remember'),
);
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

const helpDialog = new HelpDialog(
  byId<HTMLDialogElement>('help-dialog'),
  windowChrome,
);
const shell = new TauriShell();
const connectingDialog = new ConnectingDialog(
  byId<HTMLDialogElement>('connecting-dialog'),
  byId('connecting-what'),
);
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
for (const id of ['connect-button', 'reconnect-button']) {
  byId(id).addEventListener('click', () => void connectDialog.open());
}

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

async function saveUnder(name: string, dialog: SaveConnectionDialog): Promise<void> {
  let asking = name;
  for (;;) {
    const said = await controller.saveConnection(asking);
    if (said !== null) {
      dialog.finish();
      announcer.documentReturned();
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

async function saveConnection(): Promise<void> {
  const connected = controller.connectedNow;
  if (connected === null) {
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
const menuActions = {
  connect: () => void connectDialog.open(),
  newConnection: () => void newConnectionDialog.open(),
  saveConnection: () => void saveConnection(),
  exit: () => void shell.exit(),
  help: () => helpDialog.open(),
  about: () => void aboutDialog.open(),
};
const menuBarRegion = byId('menu-bar-region');
void shell.platform().then((os) => {
  applyPlatformText(document, os);
  if (os !== 'windows') {
    installSystemMenu(new TauriSystemMenu(), menuActions, windowChrome);
    return;
  }
  installMenuBar(
    byId('menu-bar'),
    menuActions,
    // Not the edit field: it is hidden while no session is live, and focusing a hidden
    // input does nothing.
    windowChrome,
  );
  // Reveal only after it is wired: a menu bar in the accessibility tree that does not yet
  // answer F10 is silent to a listener who presses it.
  menuBarRegion.hidden = false;
});

bindKeys(
  controller,
  byId<HTMLFormElement>('command-form'),
  commandInput,
  () => helpDialog.open(),
  farEndInput,
);
void controller.start().then(async () => {
  if (await controller.carryOutTheLaunchSwitch()) {
    await afterConnecting();
  }
});
