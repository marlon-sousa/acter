// Role: adapter (DOM) — keeping Tab inside a modal dialog.
//
// **The platform does not do this, and it was measured twice.** A modal `<dialog>` is
// announced as one and answers Escape, so it is easy to assume it also cycles Tab. It does
// not: Chromium sends focus from the last control to the dialog's own document. NVDA met it
// first in the About dialog on 2026-08-24, where one control meant Tab dropped the reader
// back into browse mode and took a second Escape to leave; and again in the Connect dialog
// on 2026-08-26, where Tab past Cancel announced "dialog Connect" instead of returning to
// the list of kinds.
//
// It lives in its own module because two dialogs need it and a third will: two copies of a
// focus rule are two things that can drift, and the one that drifts is the one nobody is
// testing that week.

/// Everything the platform will put focus on, in document order.
const FOCUSABLE =
  'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * Answer a `Tab` inside `dialog` by moving to the next focusable control, wrapping at both
 * ends. Any other key is left alone.
 *
 * Call it from the dialog's own `keydown` listener. It cycles explicitly rather than
 * relying on the number of controls, so a dialog that gains or loses one keeps working.
 */
export function keepTabInside(dialog: HTMLElement, event: KeyboardEvent): void {
  if (event.key !== 'Tab') {
    return;
  }
  // **Disabled controls are filtered out here rather than in the selector**, for two
  // reasons. A disabled button still matches `button` and cannot take focus, so cycling
  // onto one leaves focus where it was and swallows the key — measured with NVDA on
  // 2026-08-26, where Tab out of the last field of the SSH form did nothing at all for as
  // long as Connect was disabled by the form being incomplete. And `:not(:disabled)` in the
  // selector list cost document order: the walk came back grouped by selector, buttons
  // before the combo box that precedes them on screen, which is a different bug in the same
  // line. Filtering afterwards keeps the order the document has.
  const focusable = Array.from(
    dialog.querySelectorAll<HTMLElement>(FOCUSABLE),
  ).filter((control) => !(control as HTMLButtonElement).disabled);
  // **A dialog with no controls at all keeps the key too**, for the reason the
  // single-control case below swallows it: letting Tab through drops the reader into the
  // dialog's own document, which is the thing this function exists to prevent. The
  // connecting dialog is one of these — it holds a sentence and nothing to press, because
  // there is no way to call off a connection in flight.
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const at = focusable.indexOf(dialog.ownerDocument.activeElement as HTMLElement);
  const step = event.shiftKey ? -1 : 1;
  const next = focusable[(at + step + focusable.length) % focusable.length];
  if (next === undefined) {
    return;
  }
  // **Tab is answered by doing nothing when there is nowhere to go** — reported by the
  // user on 2026-08-26 against the failure modal, whose only control is OK. Cycling a
  // single-control dialog moves focus to the control it is already on, which a reader
  // announces again: Tab appears to do something, and what it does is repeat itself.
  // Swallowing the key is the honest answer, and it is what keeps the reader from being
  // dropped into the dialog's document, which is why this function exists at all.
  event.preventDefault();
  if (next !== dialog.ownerDocument.activeElement) {
    next.focus();
  }
}
