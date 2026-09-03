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
  escapeToCommandLine(): void {
    this.escaped += 1;
  }
  editFieldHasSelection(): boolean {
    return this.selection;
  }
  owners: number = 0;
  toggleLineOwner(): Promise<void> {
    this.owners += 1;
    return Promise.resolve();
  }
  pastes: string[] = [];
  pasteToFarEnd(text: string): Promise<void> {
    this.pastes.push(text);
    return Promise.resolve();
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
    this.owners = 0;
    this.pastes = [];
  }
}

function keydown(
  target: EventTarget,
  key: string,
  modifiers: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {},
): boolean {
  const event = new KeyboardEvent('keydown', {
    key,
    ctrlKey: modifiers.ctrl ?? false,
    shiftKey: modifiers.shift ?? false,
    altKey: modifiers.alt ?? false,
    metaKey: modifiers.meta ?? false,
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
  '<form id="command-form"><input id="command-input"></form>' +
  '<span id="far-end-input" contenteditable="true" role="textbox"></span>' +
  '<div id="results"></div>';
const controller = new StubController();
const editField = document.getElementById('command-input') as HTMLInputElement;
const farEndField = document.getElementById('far-end-input') as HTMLElement;
const results = document.getElementById('results') as HTMLElement;
let helpOpened = 0;
bindKeys(
  controller as unknown as AppController,
  document.getElementById('command-form') as HTMLFormElement,
  editField,
  () => {
    helpOpened += 1;
  },
  farEndField,
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

  // **Escape at the far end's line is the far end's** (spec 28). There it leaves insert mode
  // in `vi`, closes a completion menu in `readline` and cancels a `gh` prompt — so the field
  // consumes it, and the document listener reads `defaultPrevented` rather than asking
  // anybody which mode is on.
  it('leaves Escape alone when the far end line consumed it', () => {
    keydown(farEndField, 'Escape');

    expect(controller.escaped).toBe(0);
    expect(controller.reported).toEqual([
      { key: 'Escape', ctrl: false, shift: false, alt: false },
    ]);
  });

  // And from anywhere else it is still the way back to the command line, which is what
  // roadmap 28.3 is about: the buffer has to have a way out in both states.
  it('still returns from the results buffer while the far end owns the line', () => {
    keydown(results, 'Escape');

    expect(controller.escaped).toBe(1);
  });
});

// **Ctrl+D leaving the page is the whole of roadmap 23.5.** The path existed end to end —
// the intent, the binding, PowerShell's measured `exit` — and `isReportable` answered true
// for `Ctrl+C` and nothing else, so a listener pressing it in a real session met silence and
// a session that was still there (measured 2026-08-25, NVDA 2026.1.1, silent capture).
describe('Ctrl+D from the edit field (23.5)', () => {
  it('reports the keystroke and prevents the browser default', () => {
    const prevented = keydown(editField, 'd', { ctrl: true });

    expect(controller.reported).toEqual([
      { key: { Char: 'd' }, ctrl: true, shift: false, alt: false },
    ]);
    expect(prevented).toBe(true);
  });

  it('does not report a plain d, or Ctrl+Shift+D', () => {
    keydown(editField, 'd');
    keydown(editField, 'D', { ctrl: true, shift: true });

    expect(controller.reported).toEqual([]);
  });
});

// **Ctrl+Shift+K is layer 1 and is never reported as a keystroke** (DESIGN's default
// bindings). Layer 1 is Acter's in both states, which is what makes the way back always
// pressable — including from a far end that has stopped answering.
describe('Ctrl+Shift+K hands the line over and takes it back', () => {
  it('toggles from anywhere in the window, and is answered', () => {
    expect(keydown(results, 'K', { ctrl: true, shift: true })).toBe(true);

    expect(controller.owners).toBe(1);
    expect(controller.reported).toEqual([]);
  });

  it('is heard from the far end field too, so the way back is always pressable', () => {
    keydown(farEndField, 'K', { ctrl: true, shift: true });

    expect(controller.owners).toBe(1);
    expect(controller.reported).toEqual([]);
  });

  it('is not a plain Ctrl+K, which is a far end line-editing key', () => {
    keydown(farEndField, 'k', { ctrl: true });

    expect(controller.owners).toBe(0);
    expect(controller.reported).toEqual([
      { key: { Char: 'k' }, ctrl: true, shift: false, alt: false },
    ]);
  });
});

// The far-end field reports the *named* key and never the bytes: which spelling an arrow is
// depends on modes only the emulator tracks, and this side has never been able to know
// (spec 28, decision 4).
describe('the far end field', () => {
  it('reports each named key by name and prevents it', () => {
    const rows: Array<[string, string]> = [
      ['ArrowUp', 'Up'],
      ['ArrowDown', 'Down'],
      ['ArrowLeft', 'Left'],
      ['ArrowRight', 'Right'],
      ['Home', 'Home'],
      ['End', 'End'],
      ['Tab', 'Tab'],
      ['Enter', 'Enter'],
      ['Backspace', 'Backspace'],
      ['Delete', 'Delete'],
      ['Escape', 'Escape'],
    ];
    for (const [pressed, named] of rows) {
      controller.reset();

      const prevented = keydown(farEndField, pressed);

      expect(controller.reported).toEqual([
        { key: named, ctrl: false, shift: false, alt: false },
      ]);
      expect(prevented).toBe(true);
    }
  });

  it('reports a character key as a character, modifiers and all', () => {
    keydown(farEndField, 'y');
    keydown(farEndField, 'c', { ctrl: true });
    keydown(farEndField, 'b', { alt: true });

    expect(controller.reported).toEqual([
      { key: { Char: 'y' }, ctrl: false, shift: false, alt: false },
      { key: { Char: 'c' }, ctrl: true, shift: false, alt: false },
      { key: { Char: 'b' }, ctrl: false, shift: false, alt: true },
    ]);
  });

  // A key with no measured spelling goes nowhere rather than going as a guess: the far end
  // would answer it and say nothing about having done so.
  it('sends nothing for a key nobody measured, and does not prevent it', () => {
    for (const key of ['F5', 'PageUp', 'Insert', 'ScrollLock']) {
      controller.reset();

      const prevented = keydown(farEndField, key);

      expect(controller.reported).toEqual([]);
      expect(prevented).toBe(false);
    }
  });

  it('is silent while it does not have the keystroke', () => {
    keydown(editField, 'ArrowUp');

    expect(controller.reported).toEqual([]);
  });
});

// **Measured 2026-09-03**, VoiceOver 15.0 on macOS 15.0, at a real `bash` while the far end
// held the line: `Cmd+K` did not open Connect, it put a `k` on the far end's command line,
// and `Cmd+C` did not copy, it put a `c` there. Both halves of the fix are asserted here,
// and the second is the one that matters: a chord the platform owns must also go
// **unprevented**, or `Cmd+K` merely stops doing anything at all (spec 37, decision 2).
describe('a chord the platform owns (spec 37)', () => {
  it('sends nothing to the far end and lets the accelerator through', () => {
    for (const key of ['k', 'c', 'v', 'w', 'q', '/']) {
      controller.reset();

      const prevented = keydown(farEndField, key, { meta: true });

      expect(controller.reported).toEqual([]);
      expect(prevented).toBe(false);
    }
  });

  // The edit field forwards exactly two keys, and both have a `Cmd` spelling on the platform
  // where this bites — `Cmd+C` is copy and `Cmd+D` is a system chord. Neither is `Ctrl`.
  it('reports neither of the edit field two keys when the platform modifier is held', () => {
    for (const key of ['c', 'd']) {
      controller.reset();

      const prevented = keydown(editField, key, { meta: true });

      expect(controller.reported).toEqual([]);
      expect(prevented).toBe(false);
    }
  });

  // Layer 1 keeps its exact spelling, so a platform chord that happens to contain it is the
  // platform's rather than silently Acter's (spec 37, decision 3).
  it('leaves the line owner alone when the platform modifier is held', () => {
    const prevented = keydown(farEndField, 'k', { ctrl: true, shift: true, meta: true });

    expect(controller.owners).toBe(0);
    expect(controller.reported).toEqual([]);
    expect(prevented).toBe(false);
  });

  it('still toggles the line owner for the chord without it', () => {
    const prevented = keydown(farEndField, 'k', { ctrl: true, shift: true });

    expect(controller.owners).toBe(1);
    expect(prevented).toBe(true);
  });

  // The condition this spec adds removes nothing: `Ctrl` and `Alt` are what a terminal
  // carries, and in far-end-line mode plain `Ctrl+C` is the interrupt by another road.
  it('leaves Ctrl and Alt keystrokes reaching the far end untouched', () => {
    keydown(farEndField, 'c', { ctrl: true });
    keydown(farEndField, 'b', { alt: true });
    keydown(farEndField, 'a');

    expect(controller.reported).toEqual([
      { key: { Char: 'c' }, ctrl: true, shift: false, alt: false },
      { key: { Char: 'b' }, ctrl: false, shift: false, alt: true },
      { key: { Char: 'a' }, ctrl: false, shift: false, alt: false },
    ]);
  });
});
