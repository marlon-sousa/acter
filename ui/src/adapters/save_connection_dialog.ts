// Role: adapter (DOM) — the Save connection dialog: one field holding the name, and the
// two shapes it comes in.

import { keepTabInside } from './dialog_tab';
import type { Connected } from '../protocol';

export const ALSO_LATER =
  'You can also save later from the File menu, under Save connection.';

export const NOT_AGAIN = 'Do not offer to save new connections';

const WHY =
  'Give this connection a name, so you can start it again from the Connect dialog.';

const WHY_OFFERED =
  'This connection is working. Give it a name to save it, so you can start it again from ' +
  'the Connect dialog.';

/** `name` is `null` when they said no. */
export interface SaveAnswer {
  name: string | null;
  stopOffering: boolean;
}

export class SaveConnectionDialog {
  private offering = false;
  private settle: ((answer: SaveAnswer) => void) | null = null;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly why: HTMLElement,
    private readonly field: HTMLInputElement,
    private readonly alsoLater: HTMLElement,
    private readonly notAgain: HTMLInputElement,
    private readonly save: HTMLButtonElement,
    private readonly cancel: HTMLButtonElement,
    /**
     * Where focus goes on close, which must be placed by hand because the control that had
     * it at `showModal` may since have been hidden.
     */
    private readonly returnTo: { focus(): void },
  ) {
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    this.dialog.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && (event.target as HTMLElement).closest('button') === null) {
        event.preventDefault();
        if (this.nameable()) {
          this.answer(this.field.value);
        }
      }
    });
    this.field.addEventListener('input', () => this.settleButton());
    this.save.addEventListener('click', () => this.answer(this.field.value));
    this.cancel.addEventListener('click', () => this.answer(null));
    this.dialog.addEventListener('close', () => {
      this.answer(null);
      this.returnTo.focus();
    });
  }

  /**
   * `offering` is Acter's own offer after a new connection; `false` is File, Save connection.
   * Closing the dialog answers `null`, and still reports a ticked box.
   */
  ask(suggestion: string, offering: boolean): Promise<SaveAnswer> {
    this.offering = offering;
    this.why.textContent = offering ? WHY_OFFERED : WHY;
    this.alsoLater.textContent = ALSO_LATER;
    this.alsoLater.hidden = !offering;
    this.notAgain.checked = false;
    const around = this.notAgain.closest('p');
    if (around !== null) {
      around.hidden = !offering;
    }
    this.save.textContent = 'Save';
    this.cancel.textContent = offering ? 'Not now' : 'Cancel';
    this.field.value = suggestion;
    this.settleButton();
    return new Promise<SaveAnswer>((resolve) => {
      this.settle = resolve;
      this.dialog.showModal();
      this.field.focus();
      this.field.select();
    });
  }

  /** Must agree with the empty-name rule in crates/acter-core/src/entities/saved_connection.rs. */
  private nameable(): boolean {
    return this.field.value.trim() !== '';
  }

  private settleButton(): void {
    this.save.disabled = !this.nameable();
  }

  refused(): void {
    this.field.focus();
    this.field.select();
  }

  get open(): boolean {
    return this.dialog.open;
  }

  finish(): void {
    this.settle = null;
    if (this.dialog.open) {
      this.dialog.close();
    }
  }

  private answer(name: string | null): void {
    const settle = this.settle;
    if (settle === null) {
      return;
    }
    this.settle = null;
    const stopOffering = this.offering && this.notAgain.checked;
    if (name === null) {
      // A name leaves the dialog open, because the backend may still refuse it.
      if (this.dialog.open) {
        this.dialog.close();
      }
    }
    settle({ name, stopOffering });
  }
}

export function suggestion(connected: Connected): string {
  if (connected.saved_as !== null) {
    return connected.saved_as;
  }
  const after = connected.label.indexOf(': ');
  return after === -1 ? connected.label : connected.label.slice(after + 2);
}
