// Role: adapter (DOM) â€” the two small dialogs the Connect dialog opens on top of itself:
// Rename, which asks for one name, and Forget, which asks one question.

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
    this.dialog.addEventListener('keydown', (event) => {
      if (
        event.key === 'Enter' &&
        (event.target as HTMLElement).closest('button') === null
      ) {
        event.preventDefault();
        if (this.nameable()) {
          this.answer(this.field.value);
        }
      }
    });
    this.field.addEventListener('input', () => this.settleButton());
    this.rename.addEventListener('click', () => this.answer(this.field.value));
    cancel.addEventListener('click', () => this.answer(null));
    this.dialog.addEventListener('close', () => this.answer(null));
  }

  /** Resolves `null` when the dialog closes without a rename. */
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
    this.dialog.addEventListener('close', () => this.answer(false));
  }

  /** Resolves `false` for every way out except the Forget button. */
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
