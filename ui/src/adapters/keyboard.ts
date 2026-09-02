// Role: adapter (DOM) — global key handling translated into controller intents.

import type { AppController } from '../controllers/app';
import type { Key } from '../protocol';

export function bindKeys(
  controller: AppController,
  form: HTMLFormElement,
  editField: HTMLElement,
  openHelp: () => void,
  farEndField?: HTMLElement,
): void {
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    void controller.submit();
  });

  // F1, F6 and Escape are Acter's own and belong to the whole window, so they listen on
  // the document. What the *session* hears does not: see below.
  //
  // **F1 is the platform's "explain this"** and is unclaimed here — one keystroke with
  // nothing to disambiguate, which is the argument A7 made for F10 (spec A13, decision 3).
  // It is on the document rather than on the edit field because the sentence that sends a
  // user here is announced while the window may be showing anything: the buffer, the
  // Connect button of a window with no session, or nothing focused at all.
  document.addEventListener('keydown', (event) => {
    // **Ctrl+Shift+K is layer 1 and is never reported as a keystroke** (DESIGN's default
    // bindings; spec 28, decision 1). Layer 1 is always Acter's, in both line-ownership
    // states, which is what makes the way back always pressable: a user who has handed the
    // keyboard to a far end that has stopped answering can still take it back.
    if (isFarEndToggle(event)) {
      event.preventDefault();
      void controller.toggleLineOwner();
      return;
    }
    if (event.key === 'F1') {
      event.preventDefault();
      openHelp();
    } else if (event.key === 'F6') {
      event.preventDefault();
      controller.toggleFocusArea();
    } else if (event.key === 'Escape' && !controller.farEndOwnsTheLine()) {
      // Escape is contextual, and while the far end owns the line it is the far end's:
      // it leaves insert mode in `vi`, closes a completion menu in `readline`, and cancels
      // a `gh` prompt. Returning focus to an edit field the user is not using would take
      // that away and give nothing back.
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

  if (farEndField === undefined) {
    return;
  }
  // **Every key is prevented and nothing is ever inserted locally** (spec 28, decision 2).
  // The element is editable so that the reader speaks typed characters out of its own
  // text-box behaviour — a `contenteditable` that is not editable says nothing when you type
  // into it, measured — but what appears in it is only ever what the far end drew.
  farEndField.addEventListener('keydown', (event) => {
    // Layer 1 stays Acter's here as everywhere, and the document listener above has it.
    if (isLayerOne(event)) {
      return;
    }
    const key = keyOf(event);
    if (key === null) {
      // A key with no measured spelling goes nowhere rather than going as a guess: the far
      // end would answer it and say nothing about having done so. It is not prevented
      // either, so anything the platform still owns keeps working.
      return;
    }
    event.preventDefault();
    void controller.reportKey({
      key,
      ctrl: event.ctrlKey,
      shift: event.shiftKey,
      alt: event.altKey,
    });
  });
  // A paste is one invoke rather than a run of keystrokes, because only the backend knows
  // whether the far end asked for bracketed paste (spec 28, decision 10).
  farEndField.addEventListener('paste', (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData('text') ?? '';
    if (text !== '') {
      void controller.pasteToFarEnd(text);
    }
  });
  // Belt and braces for any path that reaches the content without a cancellable keydown —
  // a drop, an IME commit, the browser's own edit menu.
  farEndField.addEventListener('beforeinput', (event) => event.preventDefault());
}

// The keystroke that hands the line over and takes it back (DESIGN's default bindings).
//
// Matched on the physical letter rather than on `event.key`, which a held Shift turns into
// `K`: a binding that only fires for one of the two spellings is a binding that works until
// somebody's keyboard layout disagrees.
function isFarEndToggle(event: KeyboardEvent): boolean {
  return (
    (event.key === 'k' || event.key === 'K') &&
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey
  );
}

// DESIGN's layer 1: the `Ctrl+Shift` combinations that are Acter's own, in both states.
function isLayerOne(event: KeyboardEvent): boolean {
  return event.ctrlKey && event.shiftKey;
}

// The two keystrokes this frontend reports from the edit field, and the whole of what it
// forwards from there. Everything else it owns outright: text keys belong to the edit field,
// and DESIGN's layer 1 is Acter's own — reserved rather than free, so reporting it would
// have the session answer "unbound" for a key that is already spoken for.
//
// **`Ctrl+D` was added by roadmap 23.5**, and its absence was the whole of that entry: the
// path existed end to end — `SessionIntent::Eof`, the binding, the shell adapter's measured
// answer — and the key never left the page, so a listener pressing it in a real PowerShell
// session met silence and a session that was still there.
//
// Modifiers are matched exactly. A different combination is a different keystroke.
function isReportable(event: KeyboardEvent): boolean {
  return (
    (event.key === 'c' || event.key === 'd') &&
    event.ctrlKey &&
    !event.shiftKey &&
    !event.altKey
  );
}

// The DOM's name for a key, as the protocol spells it — or `null` for one the domain has no
// measured byte sequence for.
//
// The named keys are exactly `policies::key_bytes`' table. The frontend sends the name and
// never the bytes: which spelling an arrow is depends on modes only the emulator tracks, and
// this side has never been able to know (spec 28, decision 4).
function keyOf(event: KeyboardEvent): Key | null {
  switch (event.key) {
    case 'ArrowUp':
      return 'Up';
    case 'ArrowDown':
      return 'Down';
    case 'ArrowLeft':
      return 'Left';
    case 'ArrowRight':
      return 'Right';
    case 'Home':
      return 'Home';
    case 'End':
      return 'End';
    case 'Tab':
      return 'Tab';
    case 'Enter':
      return 'Enter';
    case 'Backspace':
      return 'Backspace';
    case 'Delete':
      return 'Delete';
    case 'Escape':
      return 'Escape';
    default:
      // A character key is one character. Everything longer is a named key nobody has
      // measured — the function keys, `PageUp`, `Insert`, the dead keys — and it goes
      // nowhere rather than going wrong.
      return [...event.key].length === 1 ? { Char: event.key } : null;
  }
}
