// Role: adapter (DOM) — the two small dialogs the Connect dialog opens on top of itself:
// Rename, which asks for one name, and Forget, which asks one question (spec 26,
// decision 15).
//
// **Two shapes in one file because they are one idea**: something is about to happen to the
// row the listener is on, and it does not happen until they say so. Splitting them would be
// two files of thirty lines each with the same focus rules written twice, which is the thing
// `dialog_tab` exists to prevent.
//
// **Forget is the one thing in this product nobody can undo**, which is why it asks at all.
// Its question names the row and says what does *not* change, because "forget" said of a
// connection is ambiguous in the frightening direction: somebody has to know that nothing
// on the far end is touched.

import { keepTabInside } from './dialog_tab';

export class RenameConnectionDialog {
  private settle: ((name: string | null) => void) | null = null;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly field: HTMLInputElement,
    private readonly rename: HTMLButtonElement,
    cancel: HTMLButtonElement,
  ) {
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    // Enter renames from the field, for the Save dialog's reason: one text box and two
    // buttons, and a listener who has typed a name has said what they want.
    this.dialog.addEventListener('keydown', (event) => {
      if (
        event.key === 'Enter' &&
        (event.target as HTMLElement).closest('button') === null
      ) {
        event.preventDefault();
        // The same question the button answers, never a way around it.
        if (this.nameable()) {
          this.answer(this.field.value);
        }
      }
    });
    // **Rename is disabled while the field is empty**, for the Save dialog's reason: the
    // only thing pressing it could do is earn a refusal.
    this.field.addEventListener('input', () => this.settleButton());
    this.rename.addEventListener('click', () => this.answer(this.field.value));
    cancel.addEventListener('click', () => this.answer(null));
    // Escape, and every other way of closing, leaves the name as it was.
    this.dialog.addEventListener('close', () => this.answer(null));
  }

  /**
   * Ask for a new name, with the old one prefilled and selected — so typing replaces it and
   * a listener can also arrow through what is there and change one word.
   */
  ask(name: string): Promise<string | null> {
    this.field.value = name;
    this.settleButton();
    return new Promise<string | null>((resolve) => {
      this.settle = resolve;
      this.dialog.showModal();
      this.field.focus();
      this.field.select();
    });
  }

  /** One condition, asked everywhere the action can start. */
  private nameable(): boolean {
    return this.field.value.trim() !== '';
  }

  private settleButton(): void {
    this.rename.disabled = !this.nameable();
  }

  private answer(name: string | null): void {
    const settle = this.settle;
    if (settle === null) {
      return;
    }
    this.settle = null;
    if (this.dialog.open) {
      this.dialog.close();
    }
    settle(name);
  }
}

export class ForgetConnectionDialog {
  private settle: ((forget: boolean) => void) | null = null;

  constructor(
    private readonly dialog: HTMLDialogElement,
    private readonly question: HTMLElement,
    forget: HTMLButtonElement,
    private readonly cancel: HTMLButtonElement,
  ) {
    this.dialog.addEventListener('keydown', (event) => keepTabInside(this.dialog, event));
    forget.addEventListener('click', () => this.answer(true));
    cancel.addEventListener('click', () => this.answer(false));
    // **Every way out that is not the Forget button keeps the connection**, which is the
    // shape every consequential dialog on this seam has: the safe answer is the one that
    // does nothing, and Escape gives it.
    this.dialog.addEventListener('close', () => this.answer(false));
  }

  /**
   * Ask the question, and answer whether they said yes.
   *
   * **The question is the dialog's description**, so a reader speaks it as the dialog opens
   * rather than leaving somebody to go and find it (ARCHITECTURE, dialogs rule 5).
   *
   * **Focus opens on Cancel**, which is this dialog's only deliberate difference from the
   * Save dialog beside it: this is the one action here nobody can undo, so the answer Enter
   * gives is the one that keeps the connection (rule 8's "where the choice is consequential"
   * applied one notch down — the destructive button is a Tab away rather than unreachable).
   */
  ask(asking: string): Promise<boolean> {
    this.question.textContent = asking;
    return new Promise<boolean>((resolve) => {
      this.settle = resolve;
      this.dialog.showModal();
      this.cancel.focus();
    });
  }

  private answer(forget: boolean): void {
    const settle = this.settle;
    if (settle === null) {
      return;
    }
    this.settle = null;
    if (this.dialog.open) {
      this.dialog.close();
    }
    settle(forget);
  }
}
