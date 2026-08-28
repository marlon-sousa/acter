// @vitest-environment jsdom
// Role: test — which keystrokes the frontend forwards to the session and which it keeps.
//
// The subject is the adapter's routing decision, so the controller is a stub recording
// what it was asked: whether the *answer* to a reported key is spoken is the controller's
// own test.

import { beforeEach, describe, expect, it } from 'vitest';

import { bindKeys } from '../../src/adapters/keyboard';
import type { AppController } from '../../src/controllers/app';
import type { KeyPress } from '../../src/protocol';

class StubController {
  reported: KeyPress[] = [];
  toggled = 0;
  escaped = 0;
  submitted = 0;
  /** What the edit field answers about a selection; the adapter's one input. */
  selection = false;

  submit(): Promise<void> {
    this.submitted += 1;
    return Promise.resolve();
  }
  toggleFocusArea(): void {
    this.toggled += 1;
  }
  escapeToEditField(): void {
    this.escaped += 1;
  }
  editFieldHasSelection(): boolean {
    return this.selection;
  }
  reportKey(press: KeyPress): Promise<void> {
    this.reported.push(press);
    return Promise.resolve();
  }

  reset(): void {
    this.reported = [];
    this.toggled = 0;
    this.escaped = 0;
    this.submitted = 0;
    this.selection = false;
  }
}

function keydown(
  target: EventTarget,
  key: string,
  modifiers: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {},
): boolean {
  const event = new KeyboardEvent('keydown', {
    key,
    ctrlKey: modifiers.ctrl ?? false,
    shiftKey: modifiers.shift ?? false,
    altKey: modifiers.alt ?? false,
    bubbles: true,
    cancelable: true,
  });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

// bindKeys registers on `document`, which jsdom keeps for the whole file: binding per
// test would leave every earlier test's listener attached, and a stale one calling
// preventDefault would answer for the one under test. So it is bound once and the stub
// is reset instead.
document.body.innerHTML =
  '<form id="command-form"><input id="command-input"></form><div id="results"></div>';
const controller = new StubController();
const editField = document.getElementById('command-input') as HTMLInputElement;
const results = document.getElementById('results') as HTMLElement;
let helpOpened = 0;
bindKeys(
  controller as unknown as AppController,
  document.getElementById('command-form') as HTMLFormElement,
  editField,
  () => {
    helpOpened += 1;
  },
);

beforeEach(() => {
  controller.reset();
  helpOpened = 0;
});

// **F1 belongs to the window, not to a control** (spec A13, decision 3). The sentence
// that sends a user here is announced while the window may be showing anything, so the
// key has to work from the buffer and from a window with no edit field at all — which is
// what listening on the document buys, and what these two assert.
describe('F1 opens Help (A13)', () => {
  it('opens it from the edit field, and answers the key', () => {
    const prevented = keydown(editField, 'F1');

    expect(helpOpened).toBe(1);
    expect(prevented).toBe(true);
  });

  it('opens it from the results buffer too', () => {
    keydown(results, 'F1');

    expect(helpOpened).toBe(1);
  });
});

describe('Ctrl+C from the edit field (A3.2)', () => {
  it('reports the keystroke and prevents the empty native copy', () => {
    const prevented = keydown(editField, 'c', { ctrl: true });

    expect(controller.reported).toEqual([
      { key: { Char: 'c' }, ctrl: true, shift: false, alt: false },
    ]);
    expect(prevented).toBe(true);
  });

  // The half of DESIGN's layer 2 sentence that never reaches the backend: over a
  // selection this is the platform's copy, so the key is neither reported nor prevented
  // — preventing it would break the copy just as surely as reporting it would.
  it('leaves the native copy alone when the field holds a selection', () => {
    controller.selection = true;

    const prevented = keydown(editField, 'c', { ctrl: true });

    expect(controller.reported).toEqual([]);
    expect(prevented).toBe(false);
  });

  it('does not report a plain c, which is text the field owns', () => {
    keydown(editField, 'c');

    expect(controller.reported).toEqual([]);
  });

  // Layer 1 is Acter's own and reserved rather than free. Reporting it would have the
  // session answer "unbound" for a key that is already spoken for.
  it('does not report Ctrl+Shift+C or Ctrl+Alt+C', () => {
    keydown(editField, 'C', { ctrl: true, shift: true });
    keydown(editField, 'c', { ctrl: true, alt: true });

    expect(controller.reported).toEqual([]);
  });
});

// The rule DESIGN states and this adapter enforces by construction: only the edit field
// carries the listener, so a keystroke anywhere else is not the session's to hear. In the
// results buffer Ctrl+C is the screen reader's own copy command — in NVDA's browse mode
// it never reaches the page at all — and a binding that cannot be pressed is worse than
// no binding.
describe('Ctrl+C outside the edit field', () => {
  it('is not reported from the results buffer', () => {
    const prevented = keydown(results, 'c', { ctrl: true });

    expect(controller.reported).toEqual([]);
    expect(prevented).toBe(false);
  });

  it('is not reported from the document at large', () => {
    const prevented = keydown(document, 'c', { ctrl: true });

    expect(controller.reported).toEqual([]);
    expect(prevented).toBe(false);
  });
});

describe('the keys the frontend keeps', () => {
  // F6 and Escape are Acter's own and belong to the whole window, so unlike Ctrl+C they
  // are still heard wherever focus happens to be.
  it('F6 toggles the focus area and is prevented, from anywhere', () => {
    expect(keydown(results, 'F6')).toBe(true);
    expect(controller.toggled).toBe(1);
    expect(controller.reported).toEqual([]);
  });

  it('Escape returns to the edit field', () => {
    keydown(results, 'Escape');

    expect(controller.escaped).toBe(1);
    expect(controller.reported).toEqual([]);
  });
});
