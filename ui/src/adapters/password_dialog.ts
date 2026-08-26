// Role: adapter (DOM) — the credential dialog: a masked field, and nothing else that could
// carry a password anywhere.
//
// **Why it is not the session's edit field** (spec B9, decision 4). A password typed into
// the ordinary command line would be rendered into the buffer and read aloud by the screen
// reader — the failure DESIGN names as an open question — and it would reach the transcript
// and the debug event recorder on its way. So authentication input is its own dialog, and
// the value goes straight to the backend on the connect surface, which is not the surface
// the debug recorder wraps.
//
// **The field is cleared the moment the value is handed over**, so a password does not sit
// in the DOM of a window that stays open for hours afterwards.

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

  /**
   * Asks for a password, and resolves with it or with the decision not to give one.
   *
   * **Not giving one is a decision rather than a failure**, and it is what every way of
   * closing this dialog except the submit button means.
   */
  ask(
    question: Extract<ConnectQuestion, { question: 'Password' }>,
  ): Promise<ConnectAnswer> {
    this.dialog.setAttribute('aria-label', TITLE);
    // **What is being signed in to, said before the field.** A listener answering two
    // connections, or one that reappeared, is otherwise typing a password into a dialog
    // that could belong to anything.
    const asking = `Password for ${question.user} at ${question.host}.`;
    // A second prompt with no explanation is indistinguishable from the first one not
    // having been submitted, which is precisely the confusion this product exists to
    // remove (spec B9, decision 4).
    this.prompt.textContent = question.again
      ? `${asking} The password already tried was not accepted.`
      : asking;
    this.field.value = '';

    return new Promise<ConnectAnswer>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        const given = this.dialog.returnValue === 'submit' ? this.field.value : null;
        // Cleared before the value leaves this function, so nothing is left in the
        // document for a later reader — or a later screenshot — to find.
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
