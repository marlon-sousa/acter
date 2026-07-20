// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
import { BeepAudio } from './adapters/beep';
import { BufferDom } from './adapters/buffer';
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

const editField = new EditFieldDom(byId<HTMLInputElement>('command-input'));
const buffer = new BufferDom(byId('results'));
const announcer = new AnnouncerDom(byId('announcer'));
const beep = new BeepAudio();
const controller = new AppController(
  new TauriBackend(),
  editField,
  buffer,
  announcer,
  beep,
);

bindKeys(controller, byId<HTMLFormElement>('command-form'));
// The fake is the default backend, connected automatically on load with no user
// action (decision 9): attach the session so scenario events start flowing.
void controller.attach();
editField.focus();
