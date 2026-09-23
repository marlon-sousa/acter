// Role: adapter (DOM) — the Help dialog: open it modally, and put focus back where the
// window keeps it when it closes.

import { keepTabInside } from './dialog_tab';
import type { HelpView } from '../ports/help_view';

const TOP = 'help-what-acter-is';


export class HelpDialog implements HelpView {
  private comingBackTo: { focus(): void };

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly returnTo: { focus(): void },
  ) {
    this.comingBackTo = returnTo;
    this.dialog.addEventListener('close', () => this.comingBackTo.focus());
    this.dialog
      .querySelector('#help-close')
      ?.addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('keydown', (event) =>
      keepTabInside(this.dialog, event),
    );
  }

  /**
   * Focus must move in the same turn as `showModal`: focusing 100 ms later made NVDA 2026.1.1
   * read the whole topic aloud on every opening.
   */
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void {
    if (this.dialog.open) {
      return;
    }
    this.comingBackTo = options?.returnTo ?? this.returnTo;
    this.dialog.showModal();
    this.dialog.querySelector<HTMLElement>(`#${options?.topic ?? TOP}`)?.focus();
  }
}
