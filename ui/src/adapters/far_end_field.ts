// Role: adapter (DOM) — the far end's command line, as an ARIA text box whose text and
// caret Acter writes and whose every key Acter prevents; the NVDA measurements that chose this
// shape are in docs/specs/28-far-end-line-mode.md.

import type { FarEndFieldView } from '../ports/far_end_field_view';

// Measured working with NVDA 2026.1.1, which reads the selection on the caret event the write fires.
const SELECTION_MS = 120;

export class FarEndFieldDom implements FarEndFieldView {
  private selected: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly field: HTMLElement,
    private readonly container: HTMLElement,
  ) {}

  // `text` is null when only the caret moved; writing the same string back is a change the reader announces.
  render(text: string | null, caret: number, completed = false): void {
    const before = this.field.textContent ?? '';
    if (text !== null && before !== text) {
      this.field.textContent = text;
    }
    if (completed && text !== null && text.length > before.length && text.startsWith(before)) {
      this.announceAddition(before.length, text.length, caret);
      return;
    }
    this.placeCaret(caret);
  }

  // Only a pure append is selected; the selection is how NVDA 2026.1.1 announces a completion.
  private announceAddition(from: number, to: number, caret: number): void {
    this.drop();
    const node = this.field.firstChild;
    if (node === null || node.nodeType !== Node.TEXT_NODE) {
      this.placeCaret(caret);
      return;
    }
    const selection = this.field.ownerDocument.defaultView?.getSelection();
    if (selection === undefined || selection === null) {
      this.placeCaret(caret);
      return;
    }
    const range = this.field.ownerDocument.createRange();
    range.setStart(node, from);
    range.setEnd(node, to);
    selection.removeAllRanges();
    selection.addRange(range);
    this.selected = setTimeout(() => this.placeCaret(caret), SELECTION_MS);
  }

  private drop(): void {
    if (this.selected !== undefined) {
      clearTimeout(this.selected);
      this.selected = undefined;
    }
  }

  show(showing: boolean): void {
    this.container.hidden = !showing;
  }

  focus(): void {
    this.field.focus();
  }

  isFocused(): boolean {
    return document.activeElement === this.field;
  }

  // An empty contenteditable has no text node, and an offset into a missing node throws.
  private placeCaret(caret: number): void {
    this.drop();
    const selection = this.field.ownerDocument.defaultView?.getSelection();
    if (selection === undefined || selection === null) {
      return;
    }
    const range = this.field.ownerDocument.createRange();
    const node = this.field.firstChild;
    if (node === null || node.nodeType !== Node.TEXT_NODE) {
      range.selectNodeContents(this.field);
      range.collapse(true);
    } else {
      const length = node.textContent?.length ?? 0;
      // The far end's cursor is a screen column and may sit past the last character.
      range.setStart(node, Math.max(0, Math.min(caret, length)));
      range.collapse(true);
    }
    selection.removeAllRanges();
    selection.addRange(range);
  }
}
