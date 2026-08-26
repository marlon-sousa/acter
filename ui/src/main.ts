// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
import { BeepAudio } from './adapters/beep';
import { BufferDom } from './adapters/buffer';
import { installDebugRecorder } from './adapters/debug_recorder';
import { AboutDialog } from './adapters/about_dialog';
import { EditFieldDom } from './adapters/edit_field';
import { bindKeys } from './adapters/keyboard';
import { installMenuBar } from './adapters/menu_bar';
import { WindowChrome } from './adapters/window_chrome';
import { AppController } from './controllers/app';
import { TauriBackend, TauriConnect, TauriShell } from './routers/tauri';

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
const windowChrome = new WindowChrome(
  byId('window-title'),
  byId('connection-status'),
  document,
  (title) => void shell.setTitle(title),
);
const controller = new AppController(
  installDebugRecorder(new TauriBackend()),
  new TauriConnect(),
  editField,
  buffer,
  announcer,
  beep,
  windowChrome,
);

// The menu bar is in the document rather than in the window frame, and F10 is the way
// into it (spec A7). Its two items are handed the things they act on, so the bar itself
// knows about neither the shell nor the dialog.
const shell = new TauriShell();
const aboutDialog = new AboutDialog(
  byId<HTMLDialogElement>('about-dialog'),
  shell,
  editField,
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
  menuBarRegion.hidden = false;
  installMenuBar(
    byId('menu-bar'),
    {
      exit: () => void shell.exit(),
      about: () => void aboutDialog.open(),
    },
    editField,
  );
});

// The edit field is passed because the session hears a keystroke only while that field
// has focus (DESIGN, layer 2), and the adapter enforces that by listening on the element
// rather than on the document.
bindKeys(controller, byId<HTMLFormElement>('command-form'), commandInput);
// What this window opens onto: the session the launch brought, or nothing at all — which
// since B7 is the ordinary case, and which the controller announces rather than leaving a
// listener in front of a window that says nothing (spec B7, decision 3). Naming the far end
// is part of the same call now: a session can be replaced while the window is open, so the
// title comes from the connection rather than from what the process was started with.
void controller.start();
editField.focus();
