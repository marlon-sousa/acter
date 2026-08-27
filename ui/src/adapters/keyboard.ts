// Role: adapter (DOM) — global key handling translated into controller intents.

import type { AppController } from '../controllers/app';

export function bindKeys(
  controller: AppController,
  form: HTMLFormElement,
  editField: HTMLElement,
  openHelp: () => void,
): void {
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    void controller.submit();
  });

  // F1, F6 and Escape are Acter's own and belong to the whole window, so they listen on
  // the document. What the *session* hears does not: see below.
  //
  // **F1 is the platform's "explain this"** and is unclaimed here — one keystroke with
  // nothing to disambiguate, which is the argument A7 made for F10 (spec A12, decision 3).
  // It is on the document rather than on the edit field because the sentence that sends a
  // user here is announced while the window may be showing anything: the buffer, the
  // Connect button of a window with no session, or nothing focused at all.
  document.addEventListener('keydown', (event) => {
    if (event.key === 'F1') {
      event.preventDefault();
      openHelp();
    } else if (event.key === 'F6') {
      event.preventDefault();
      controller.toggleFocusArea();
    } else if (event.key === 'Escape') {
      controller.escapeToEditField();
    }
  });

  // Bound to the edit field rather than the document, which is the whole of DESIGN's
  // "the session hears a keystroke only while the edit field has focus": a keydown
  // reaches this listener only when the field already has focus, so the rule holds by
  // construction instead of by a condition a later edit can forget.
  //
  // The results buffer deliberately has no such listener. There, `Ctrl+C` is the screen
  // reader's own copy command — in NVDA's browse mode it is answered by the reader and
  // never delivered here at all — and a binding that cannot be pressed is worse than no
  // binding, because it reads as an interrupt the user can rely on (DESIGN, layer 2).
  editField.addEventListener('keydown', (event) => {
    if (!isReportable(event)) {
      return;
    }
    // Over a selection the platform still owns this keystroke: it is the native copy, so
    // it is neither prevented nor reported.
    if (controller.editFieldHasSelection()) {
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
