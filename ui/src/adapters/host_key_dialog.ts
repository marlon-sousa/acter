// Role: adapter (DOM) — the host-key dialog: the security decision in SSH, put to a person
// who cannot see a wall of warning text.

import { keepTabInside } from './dialog_tab';
import { readableField } from './readable_field';
import type { ConnectAnswer, ConnectQuestion } from '../protocol';

const UNKNOWN_TITLE = 'Unknown server';
const CHANGED_TITLE = 'Warning: this server has changed';

const CHANGED_WARNING =
  'The server at this address is not the one Acter connected to before. Either it was rebuilt, or something is pretending to be it. If you were not expecting this, do not connect.';

const UNKNOWN_EXPLANATION =
  'Acter has never connected to this server before, so it has nothing to compare its identity against. Check the fingerprint below against one you trust before connecting.';

export class HostKeyDialog {
  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly title: HTMLElement,
    private readonly summary: HTMLElement,
    private readonly body: HTMLElement,
  ) {
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
    this.dialog.addEventListener('keydown', (event) => this.noDefaultAction(event));
  }

  /** Resolves `GiveUp` for every way out except the trust button. */
  ask(question: Extract<ConnectQuestion, { question: 'HostKey' }>): Promise<ConnectAnswer> {
    const document = this.body.ownerDocument;
    const changed = question.recorded !== null;
    this.title.textContent = changed ? CHANGED_TITLE : UNKNOWN_TITLE;
    this.dialog.setAttribute(
      'aria-label',
      changed ? CHANGED_TITLE : UNKNOWN_TITLE,
    );

    this.summary.textContent = [
      `${question.host}, port ${question.port}.`,
      changed ? CHANGED_WARNING : UNKNOWN_EXPLANATION,
      question.aside,
    ]
      .filter((part) => part !== null && part !== '')
      .join(' ');

    const said = document.createElement('div');
    if (changed) {
      said.append(
        readableField(
          document,
          'host-key-recorded',
          'Fingerprint Acter recorded before',
          question.recorded ?? '',
        ),
      );
    }
    said.append(
      readableField(
        document,
        'host-key-offered',
        'Fingerprint this server is offering now',
        question.fingerprint,
      ),
    );
    this.body.replaceChildren(said);

    return new Promise<ConnectAnswer>((resolve) => {
      const settle = (): void => {
        this.dialog.removeEventListener('close', settle);
        resolve(
          this.dialog.returnValue === 'trust'
            ? { answer: 'Trust' }
            : { answer: 'GiveUp' },
        );
      };
      this.dialog.addEventListener('close', settle);
      this.dialog.returnValue = '';
      this.dialog.showModal();
      this.offered()?.focus();
    });
  }

  private offered(): HTMLElement | null {
    return this.dialog.querySelector<HTMLElement>('#host-key-offered');
  }

  /**
   * Enter outside a button must never pick an outcome, which is what makes starting focus
   * on a field safe.
   */
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
