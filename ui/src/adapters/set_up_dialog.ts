// Role: adapter (DOM) — the dialog that discloses the one command Acter would run inside a
// session once that session is established.
//
// Prose in this dialog belongs in its description and never in a tab stop, because a reader
// cannot arrow through prose inside `role="application"`.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

const COMMAND = 'set-up-command';

const SET_UP = 'set-up';

const COMMAND_LABEL = 'This is the command Acter will run for you';

export class SetUpDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly summary: HTMLElement,
    private readonly body: HTMLElement,
    private readonly remember: HTMLInputElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.dialog.addEventListener('keydown', (event) => this.noDefaultAction(event));
  }

  /** Resolves `GiveUp` for every way out except "Run command". */
  ask(
    question: Extract<ConnectQuestion, { question: 'SetUpSession' }>,
  ): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    this.summary.textContent = `${question.detected} ${question.offer} ${question.refusal}`;

    const said = document.createElement('div');
    said.append(
      readableField(document, COMMAND, COMMAND_LABEL, question.command),
    );
    this.body.replaceChildren(said);

    this.remember.checked = false;

    return new Promise<ConnectAnswer>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === SET_UP
            ? { answer: 'SetUpSession', remember: this.remember.checked }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      this.dialog.querySelector<HTMLElement>(`#${COMMAND}`)?.focus();
    });
  }

  /** See `noDefaultAction` in host_key_dialog.ts. */
  private noDefaultAction(event: KeyboardEvent): void {
    if (event.key !== 'Enter') {
      return;
    }
    if ((event.target as HTMLElement).closest('button') !== null) {
      return;
    }
    event.preventDefault();
  }
}
