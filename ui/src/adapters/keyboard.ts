// Role: adapter (DOM) — global key handling translated into controller intents.

import type { AppController } from '../controllers/app';

export function bindKeys(controller: AppController, form: HTMLFormElement): void {
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    void controller.submit();
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'F6') {
      event.preventDefault();
      controller.toggleFocusArea();
    } else if (event.key === 'Escape') {
      controller.escapeToEditField();
    }
  });
}
