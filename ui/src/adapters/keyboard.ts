// Role: adapter (DOM) — global key handling translated into controller intents.

import type { AppController } from '../controllers/app';

export function bindKeys(controller: AppController, form: HTMLFormElement): void {
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    void controller.submit();
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'F6') {
      event.preventDefault();
      controller.toggleFocusArea();
    } else if (event.key === 'Escape') {
      controller.escapeToEditField();
    } else if (isReportable(event)) {
      // Over a selection the platform still owns this keystroke — it is the native
      // copy — so it is neither prevented nor reported. Which area's selection counts
      // is the controller's to answer, because DESIGN's layer 2 makes the rule
      // per-focus and the two areas do not answer it the same way.
      if (controller.focusedAreaHasSelection()) {
        return;
      }
      // Nothing native is left to run, so stop the browser attempting an empty copy.
      event.preventDefault();
      void controller.reportKey({
        key: { Char: event.key },
        ctrl: event.ctrlKey,
        shift: event.shiftKey,
        alt: event.altKey,
      });
    }
  });
}

// The one keystroke this frontend reports today, and the whole of what it forwards.
// Everything else it owns outright: text keys belong to the edit field, and DESIGN's
// layer 1 (Ctrl+Shift+letter) is Acter's own — reserved rather than free, so reporting
// it would have the session answer "unbound" for a key that is already spoken for.
//
// Modifiers are matched exactly. A different combination is a different keystroke.
function isReportable(event: KeyboardEvent): boolean {
  return event.key === 'c' && event.ctrlKey && !event.shiftKey && !event.altKey;
}
