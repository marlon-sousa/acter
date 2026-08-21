// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
import { BeepAudio } from './adapters/beep';
import { BufferDom } from './adapters/buffer';
import { installDebugRecorder } from './adapters/debug_recorder';
import { EditFieldDom } from './adapters/edit_field';
import { bindKeys } from './adapters/keyboard';
import { AppController } from './controllers/app';
import { TauriBackend } from './routers/tauri';

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
const controller = new AppController(
  installDebugRecorder(new TauriBackend()),
  editField,
  buffer,
  announcer,
  beep,
);

// The edit field is passed because the session hears a keystroke only while that field
// has focus (DESIGN, layer 2), and the adapter enforces that by listening on the element
// rather than on the document.
bindKeys(controller, byId<HTMLFormElement>('command-form'), commandInput);
// The fake is the default backend, connected automatically on load with no user
// action (decision 9): attach the session so scenario events start flowing.
void controller.attach();
editField.focus();
