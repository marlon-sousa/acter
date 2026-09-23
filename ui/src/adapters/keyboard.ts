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

  document.addEventListener('keydown', (event) => {
    // Checked first: the way back from a far end must stay pressable in both states.
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
    } else if (event.key === 'Escape' && !event.defaultPrevented) {
      // The far-end field's own listener runs first and prevents an Escape that is the far end's.
      controller.escapeToCommandLine();
    }
  });

  // On the edit field, not the document, so the session hears keys only while the field has focus.
  editField.addEventListener('keydown', (event) => {
    if (!isReportable(event)) {
      return;
    }
    // Over a selection the key is the native copy: neither prevented nor reported.
    if (controller.editFieldHasSelection()) {
      return;
    }
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
  // The field must stay editable: a contenteditable that is not editable says nothing when typed into.
  farEndField.addEventListener('keydown', (event) => {
    if (isLayerOne(event)) {
      return;
    }
    // Must return before preventDefault, so the platform's chord still works.
    if (platformOwns(event)) {
      return;
    }
    const key = keyOf(event);
    if (key === null) {
      // Neither sent nor prevented.
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
  farEndField.addEventListener('paste', (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData('text') ?? '';
    if (text !== '') {
      void controller.pasteToFarEnd(text);
    }
  });
  // A drop, an IME commit and the edit menu reach the content without a cancellable keydown.
  farEndField.addEventListener('beforeinput', (event) => event.preventDefault());
}

function isFarEndToggle(event: KeyboardEvent): boolean {
  return (
    (event.key === 'k' || event.key === 'K') &&
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !platformOwns(event)
  );
}

// The DOM's `metaKey` is Command or the Windows key, never the terminal's Meta, which is Alt.
function platformOwns(event: KeyboardEvent): boolean {
  return event.metaKey;
}

function isLayerOne(event: KeyboardEvent): boolean {
  return event.ctrlKey && event.shiftKey;
}

function isReportable(event: KeyboardEvent): boolean {
  return (
    (event.key === 'c' || event.key === 'd') &&
    event.ctrlKey &&
    !event.shiftKey &&
    !event.altKey &&
    !platformOwns(event)
  );
}

// Null for a key with no measured byte sequence; the named keys must match `policies::key_bytes`.
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
      return [...event.key].length === 1 ? { Char: event.key } : null;
  }
}
