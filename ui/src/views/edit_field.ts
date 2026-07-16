// Role: adapter (DOM) — the command input element.

import type { EditFieldView } from './ports';

export class EditFieldDom implements EditFieldView {
  constructor(private readonly input: HTMLInputElement) {}

  value(): string {
    return this.input.value;
  }

  clear(): void {
    this.input.value = '';
  }

  focus(): void {
    this.input.focus();
  }

  isFocused(): boolean {
    return document.activeElement === this.input;
  }
}
