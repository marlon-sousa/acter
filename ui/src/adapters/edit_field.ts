// Role: adapter (DOM) — the command input element.

import type { EditFieldView } from '../ports/edit_field_view';

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

  // window.getSelection() does not report a range inside a text field.
  hasSelection(): boolean {
    return this.input.selectionStart !== this.input.selectionEnd;
  }
}
