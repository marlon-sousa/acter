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
  /** What the focused area answers about a selection; the adapter's one input. */
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
  focusedAreaHasSelection(): boolean {
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

/** A keydown as the document sees it, and whether anything called preventDefault. */
function press(
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
  document.dispatchEvent(event);
  return event.defaultPrevented;
}

// bindKeys registers on `document`, which jsdom keeps for the whole file: binding per
// test would leave every earlier test's listener attached, and a stale one calling
// preventDefault would answer for the one under test. So it is bound once and the stub
// is reset instead.
document.body.innerHTML = '<form id="command-form"></form>';
const controller = new StubController();
bindKeys(
  controller as unknown as AppController,
  document.getElementById('command-form') as HTMLFormElement,
);

beforeEach(() => {
  controller.reset();
});

describe('Ctrl+C (A3.2)', () => {
  it('reports the keystroke and prevents the empty native copy', () => {
    const prevented = press('c', { ctrl: true });

    expect(controller.reported).toEqual([
      { key: { Char: 'c' }, ctrl: true, shift: false, alt: false },
    ]);
    expect(prevented).toBe(true);
  });

  // The half of DESIGN's layer 2 sentence that never reaches the backend: over a
  // selection this is the platform's copy, so the key is neither reported nor prevented
  // — preventing it would break the copy just as surely as reporting it would.
  it('leaves the native copy alone when the focused area holds a selection', () => {
    controller.selection = true;

    const prevented = press('c', { ctrl: true });

    expect(controller.reported).toEqual([]);
    expect(prevented).toBe(false);
  });

  it('does not report a plain c, which is text the edit field owns', () => {
    press('c');

    expect(controller.reported).toEqual([]);
  });

  // Layer 1 is Acter's own and reserved rather than free. Reporting it would have the
  // session answer "unbound" for a key that is already spoken for.
  it('does not report Ctrl+Shift+C or Ctrl+Alt+C', () => {
    press('C', { ctrl: true, shift: true });
    press('c', { ctrl: true, alt: true });

    expect(controller.reported).toEqual([]);
  });
});

describe('the keys the frontend keeps', () => {
  it('F6 toggles the focus area and is prevented', () => {
    expect(press('F6')).toBe(true);
    expect(controller.toggled).toBe(1);
    expect(controller.reported).toEqual([]);
  });

  it('Escape returns to the edit field', () => {
    press('Escape');

    expect(controller.escaped).toBe(1);
    expect(controller.reported).toEqual([]);
  });
});
