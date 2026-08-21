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

  // An input's own selection, which is not the document's: window.getSelection() does
  // not report a range inside a text field at all, which is why this question is asked
  // of each area rather than once at the document level.
  hasSelection(): boolean {
    return this.input.selectionStart !== this.input.selectionEnd;
  }
}
