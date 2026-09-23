// Role: adapter (DOM) — keeping Tab inside a modal dialog.
//
// WebView2 does not cycle Tab in a modal `<dialog>`: past the last control focus goes to the
// dialog's own document, and NVDA drops back into browse mode.

const FOCUSABLE =
  'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

export function keepTabInside(dialog: HTMLElement, event: KeyboardEvent): void {
  if (event.key !== 'Tab') {
    return;
  }
  // Disabled controls, and controls inside `[hidden]`, match the selector but cannot take
  // focus, so cycling onto one would swallow Tab.
  const focusable = Array.from(
    dialog.querySelectorAll<HTMLElement>(FOCUSABLE),
  ).filter(
    (control) =>
      !(control as HTMLButtonElement).disabled && control.closest('[hidden]') === null,
  );
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
  event.preventDefault();
  if (next !== dialog.ownerDocument.activeElement) {
    next.focus();
  }
}
