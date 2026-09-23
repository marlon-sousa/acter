// Role: adapter (DOM) — a modal that says one thing and waits to be dismissed.

import { keepTabInside } from './dialog_tab';
import type { MessageView } from '../ports/message_view';

export class MessageDialog implements MessageView {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly body: HTMLElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  /** Resolves once the dialog has been dismissed. */
  show(sentence: string): Promise<void> {
    this.body.textContent = sentence;
    return new Promise<void>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve();
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.showModal();
      this.dialog.querySelector<HTMLElement>('#failed-ok')?.focus();
    });
  }
}
