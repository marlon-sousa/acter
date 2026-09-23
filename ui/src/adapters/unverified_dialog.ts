// Role: adapter (DOM) — the dialog that asks whether to start a program Windows would not
// vouch for.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

const PROGRAM = 'unverified-program';
const SIGNER = 'unverified-signer';

const START = 'start';

export class UnverifiedDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly summary: HTMLElement,
    private readonly body: HTMLElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.dialog.addEventListener('keydown', (event) => this.noDefaultAction(event));
  }

  /** Resolves `GiveUp` for every way out except the starting button. */
  ask(
    question: Extract<ConnectQuestion, { question: 'Unverified' }>,
  ): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    this.summary.textContent = `${question.label}. ${question.said}`;

    const said = document.createElement('div');
    said.append(
      readableField(document, PROGRAM, 'File Acter would start', question.program),
    );
    if (question.signer !== null) {
      said.append(
        readableField(document, SIGNER, 'Signed by', question.signer),
      );
    }
    this.body.replaceChildren(said);

    return new Promise<ConnectAnswer>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === START
            ? { answer: 'StartAnyway' }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      this.dialog.querySelector<HTMLElement>(`#${PROGRAM}`)?.focus();
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
