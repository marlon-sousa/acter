// @vitest-environment jsdom
// Role: test — which keystrokes the frontend forwards to the session and which it keeps.

import { beforeEach, describe, expect, it } from 'vitest';

import { bindKeys } from '../../src/adapters/keyboard';
import type { AppController } from '../../src/controllers/app';
import type { KeyPress } from '../../src/protocol';

class StubController {
  reported: KeyPress[] = [];
  toggled = 0;
  escaped = 0;
  submitted = 0;
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

// bindKeys listens on `document`, which jsdom keeps for the whole file, so it is bound once
// and the stub is reset per test; a listener bound per test would outlive its test.
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

  it('does not report Ctrl+Shift+C or Ctrl+Alt+C', () => {
    keydown(editField, 'C', { ctrl: true, shift: true });
    keydown(editField, 'c', { ctrl: true, alt: true });

    expect(controller.reported).toEqual([]);
  });
});

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

  it('leaves Escape alone when the far end line consumed it', () => {
    keydown(farEndField, 'Escape');

    expect(controller.escaped).toBe(0);
    expect(controller.reported).toEqual([
      { key: 'Escape', ctrl: false, shift: false, alt: false },
    ]);
  });

  it('still returns from the results buffer while the far end owns the line', () => {
    keydown(results, 'Escape');

    expect(controller.escaped).toBe(1);
  });
});

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

describe('a chord the platform owns (spec 37)', () => {
  it('sends nothing to the far end and lets the accelerator through', () => {
    for (const key of ['k', 'c', 'v', 'w', 'q', '/']) {
      controller.reset();

      const prevented = keydown(farEndField, key, { meta: true });

      expect(controller.reported).toEqual([]);
      expect(prevented).toBe(false);
    }
  });

  it('reports neither of the edit field two keys when the platform modifier is held', () => {
    for (const key of ['c', 'd']) {
      controller.reset();

      const prevented = keydown(editField, key, { meta: true });

      expect(controller.reported).toEqual([]);
      expect(prevented).toBe(false);
    }
  });

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
