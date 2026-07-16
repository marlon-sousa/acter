// Role: container (composition root) — the only place where objects are
// constructed and bound.

import { AnnouncerDom } from './adapters/announcer';
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
const controller = new AppController(new TauriBackend(), editField, buffer, announcer);

bindKeys(controller, byId<HTMLFormElement>('command-form'));
editField.focus();
