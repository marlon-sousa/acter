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
  const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE));
  if (focusable.length === 0) {
    return;
  }
  const at = focusable.indexOf(dialog.ownerDocument.activeElement as HTMLElement);
  const step = event.shiftKey ? -1 : 1;
  const next = focusable[(at + step + focusable.length) % focusable.length];
  if (next !== undefined) {
    event.preventDefault();
    next.focus();
  }
}
