// Role: adapter (DOM) — the far end's command line, as an ARIA text box whose text and
// caret Acter writes and whose every key Acter prevents.
//
// **The shape was measured before it was chosen** (spec 28, decision 2). On NVDA 2026.1.1
// through the screen-readers bridge, `user` persona, silent capture, in Edge — the engine
// this app's WebView2 runs — with a field held permanently empty as this one is at a fresh
// prompt:
//
//   - A plain `<input>` says "blank" before every arrow. Right, left, up, down, Home and End
//     each produced exactly one utterance: "em branco".
//   - **Preventing the arrows changes nothing.** The identical field with `preventDefault`
//     on every arrow said "blank" exactly as often — so the word comes from NVDA reading the
//     line *after* a caret command, not from a caret that moved, and the guess this entry was
//     told to measure was wrong.
//   - Answering into a live region does not mask it, politely or assertively: "em branco"
//     then "cargo test --all", two utterances in the same millisecond, the blank first.
//   - A focusable non-text element is silent — and **so is typing into it**: three typed
//     characters produced no speech at all, where the same three in an `<input>` were spoken.
//   - `role="textbox"` over no editable text is the worst of both: reported as read-only
//     editable text, "blank" on every arrow, and typed characters not spoken.
//
// What works, and what this is: a `<span contenteditable="true" role="textbox"
// aria-multiline="false">` whose content and caret are set from what the far end did. Landing
// on it said "command line, edit, cargo test --all" — announced as an edit field, which is
// what `role="textbox"` buys; without it NVDA says "section, multiline, editable". Right
// arrow with the caret at column 1 said "a", the character at the far end's cursor. Up arrow
// with the text replaced by `exit` said "exit". Typing `y` said "y", because the element is
// editable. A row a key emptied, and a caret past the last character, both said "blank" —
// NVDA's own word, in a vocabulary its users already have, which is why this module invents
// no strings and why the mode uses no live region at all.
//
// **It is deliberately not `role="application"`.** That role's cost is measured and recorded
// in `readable_field.ts`: inside it the arrows stop reading prose. This shape needs none of
// it — the document stays browsable, and the handoff into focus mode is still the user's own
// NVDA+Space.

import type { FarEndFieldView } from '../ports/far_end_field_view';

/**
 * How long what a completion added stays selected before the caret is collapsed onto it.
 *
 * Long enough for NVDA to have read the selection — it announces one out of
 * `EditableText.detectPossibleSelectionChange`, which runs on the caret event the write
 * fires — and short enough that nothing stale is left standing in a field whose contents
 * are the far end's rather than provisional. 120ms measured working on NVDA 2026.1.1.
 */
const SELECTION_MS = 120;

/**
 * The far end's command line.
 *
 * Nothing is ever inserted locally: every key is prevented by `keyboard.ts` and the only
 * thing that changes this element is [`render`](FarEndFieldDom.render), from what the far
 * end drew.
 */
export class FarEndFieldDom implements FarEndFieldView {
  /** The timer that drops a completion's selection, so a second one cannot outlive it. */
  private selected: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly field: HTMLElement,
    private readonly container: HTMLElement,
  ) {}

  /**
   * Put the far end's row in front of the listener, with the caret where its cursor is.
   *
   * `text` is `null` when nothing was redrawn and only the caret moved — left, right, Home
   * and End — and writing the same string back would be a text change the reader announces
   * as one. So the two cases are kept apart all the way from the domain.
   */
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

  /**
   * Leave what the completion added selected, so the reader says it, then collapse.
   *
   * **This is the inline-autocomplete pattern, and it is the one mechanism that reaches a
   * completion without a live region** (roadmap 28.4). Measured on NVDA 2026.1.1 across
   * eleven variants of this element: no role announces a programmatic content change —
   * `aria-autocomplete` is a state announced once on focus and never again, and the full
   * ARIA combobox with `aria-activedescendant` announces the completion and then silences
   * `Home`, the arrows and `Backspace` entirely, which would undo 28.1. A selection is
   * announced out of NVDA's own `detectPossibleSelectionChange`, and costs nothing else:
   * `ech` then Tab said "o  selecionado".
   *
   * **Only a pure append**, which is what a completion is. A far end that rewrites the row
   * instead has no addition to point at, and inventing one would mean speaking a diff the
   * user never saw.
   *
   * The selection is dropped again after [`SELECTION_MS`], because the field holds the far
   * end's line rather than a suggestion: leaving text selected in it would say that typing
   * replaces it, which is not true of anything here.
   */
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

  /** Cancels a pending collapse, so two completions in a row cannot cross. */
  private drop(): void {
    if (this.selected !== undefined) {
      clearTimeout(this.selected);
      this.selected = undefined;
    }
  }

  /** Whether this window is showing the far end's line at all. */
  show(showing: boolean): void {
    this.container.hidden = !showing;
  }

  focus(): void {
    this.field.focus();
  }

  isFocused(): boolean {
    return document.activeElement === this.field;
  }

  /**
   * Move the caret to a character offset, counted from the start of the row the field
   * holds.
   *
   * **A range in the text node rather than a selection offset in the element**, because an
   * empty `contenteditable` has no text node at all and an offset into one that is not there
   * throws. An empty row therefore places the caret in the element itself, which is where a
   * caret in an empty text box belongs — and is what NVDA reads as "blank".
   */
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
      // Clamped, because the far end's cursor is a screen column and the row is text: a
      // column past the last character is an ordinary state (it is where the caret sits
      // after the last thing typed) and it belongs at the end rather than nowhere.
      range.setStart(node, Math.max(0, Math.min(caret, length)));
      range.collapse(true);
    }
    selection.removeAllRanges();
    selection.addRange(range);
  }
}
