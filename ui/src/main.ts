// Role: composition root — the only place where objects are constructed and bound.

import { AppController } from './controller/app';
import { TauriBackend } from './ipc/router';
import { AnnouncerDom } from './views/announcer';
import { BufferDom } from './views/buffer';
import { EditFieldDom } from './views/edit_field';
import { bindKeys } from './views/keyboard';

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
