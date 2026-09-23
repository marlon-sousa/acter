// Role: adapter (DOM) — the credential dialog: a masked field, and nothing else that could
// carry a password anywhere.
//
// The answer must go only to the connect surface, never through `BackendApi`, which the
// debug recorder wraps.

import { keepTabInside } from './dialog_tab';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

const TITLE = 'Password';

export class PasswordDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly prompt: HTMLElement,
    private readonly field: HTMLInputElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  /** Resolves `GiveUp` for every way of closing the dialog except the submit button. */
  ask(
    question: Extract<ConnectQuestion, { question: 'Password' }>,
  ): Promise<ConnectAnswer> {
    this.dialog.setAttribute('aria-label', TITLE);
    const asking = `Password for ${question.user} at ${question.host}.`;
    this.prompt.textContent = question.again
      ? `${asking} The password already tried was not accepted.`
      : asking;
    this.field.value = '';

    return new Promise<ConnectAnswer>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        const given = this.dialog.returnValue === 'submit' ? this.field.value : null;
        this.field.value = '';
        resolve(
          given === null
            ? { answer: 'GiveUp' }
            : { answer: 'Password', secret: given },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      this.field.focus();
    });
  }
}
