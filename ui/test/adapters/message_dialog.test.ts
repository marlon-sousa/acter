// @vitest-environment jsdom
// Role: test — the modal that says one thing and waits to be dismissed.
//
// It exists because a connection error used to arrive as a live-region announcement and a
// user expected something to press OK on (reported 2026-08-26). What is pinned here is the
// two properties that make it different from an announcement: the sentence is the dialog's
// own description, so a reader says it on opening, and the promise does not settle until
// somebody has dismissed it.

import { beforeEach, describe, expect, it } from 'vitest';

import { keepTabInside } from '../../src/adapters/dialog_tab';
import { MessageDialog } from '../../src/adapters/message_dialog';

function build(): { dialog: HTMLDialogElement; message: MessageDialog } {
  document.body.innerHTML = `
    <dialog id="failed-dialog" aria-describedby="failed-why">
      <h1>Could not connect</h1>
      <p id="failed-why"></p>
      <form method="dialog"><button id="failed-ok" value="ok">OK</button></form>
    </dialog>`;
  const dialog = document.getElementById('failed-dialog') as HTMLDialogElement;
  dialog.showModal ??= function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  dialog.close ??= function close(this: HTMLDialogElement, value?: string) {
    this.open = false;
    if (value !== undefined) {
      this.returnValue = value;
    }
    this.dispatchEvent(new Event('close'));
  };
  return {
    dialog,
    message: new MessageDialog(
      dialog,
      document.getElementById('failed-why') as HTMLElement,
    ),
  };
}

describe('MessageDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('puts the sentence where a reader says it on opening', async () => {
    const { dialog, message } = build();

    const shown = message.show('Acter could not reach acter-ssh on port 2222.');

    expect(document.getElementById('failed-why')?.textContent).toBe(
      'Acter could not reach acter-ssh on port 2222.',
    );
    // Described by that element, so it is announced with the dialog rather than found.
    expect(dialog.getAttribute('aria-describedby')).toBe('failed-why');

    dialog.close('ok');
    await shown;
  });

  it('opens with focus on the button that dismisses it', async () => {
    const { dialog, message } = build();

    const shown = message.show('It did not work.');

    expect(document.activeElement?.id).toBe('failed-ok');

    dialog.close('ok');
    await shown;
  });

  // **The difference from an announcement**: it is still there until acknowledged.
  it('does not settle until it has been dismissed', async () => {
    const { dialog, message } = build();
    let dismissed = false;

    const shown = message.show('It did not work.').then(() => {
      dismissed = true;
    });

    await Promise.resolve();
    expect(dismissed).toBe(false);

    dialog.close('ok');
    await shown;
    expect(dismissed).toBe(true);
  });
});

/**
 * **Tab in a dialog with one control does nothing at all.**
 *
 * Reported by the user on 2026-08-26. `keepTabInside` exists because Chromium drops focus
 * out of a modal on the last control (A7 measured it, A8 measured it again) — but cycling
 * a single-control dialog lands on the control it started from, and a reader announces it
 * again. Tab then appears to act, and what it does is repeat itself.
 */
describe('Tab with nowhere to go', () => {
  it('is swallowed rather than re-announcing the only control', () => {
    const { dialog, message } = build();
    void message.show('It did not work.');
    const ok = document.getElementById('failed-ok') as HTMLButtonElement;
    ok.focus();
    let focused = 0;
    ok.addEventListener('focus', () => {
      focused += 1;
    });

    const tab = new KeyboardEvent('keydown', {
      key: 'Tab',
      bubbles: true,
      cancelable: true,
    });
    dialog.dispatchEvent(tab);

    // Swallowed, so the platform does not send focus to the dialog's own document...
    expect(tab.defaultPrevented).toBe(true);
    // ...and not re-focused, so nothing is announced a second time.
    expect(focused).toBe(0);
    expect(document.activeElement?.id).toBe('failed-ok');

    dialog.close('ok');
  });
});

/**
 * **A disabled control is not a place Tab can land.**
 *
 * Measured with NVDA on 2026-08-26, in the connect dialog: with Connect disabled by an
 * incomplete form, Tab out of the last field did nothing at all, because the cycle stepped
 * onto a button that cannot take focus and stayed there. Two correct changes — a button
 * that follows the form, and a Tab that stays inside the dialog — met each other exactly
 * where a user would.
 */
describe('Tab past a disabled control', () => {
  it('skips it rather than stalling on it', () => {
    document.body.innerHTML = `
      <dialog id="failed-dialog">
        <input id="first" />
        <button id="disabled-one" disabled>Cannot</button>
        <button id="last">Can</button>
      </dialog>`;
    const dialog = document.getElementById('failed-dialog') as HTMLDialogElement;
    dialog.addEventListener('keydown', (event) => keepTabInside(dialog, event));
    const first = document.getElementById('first') as HTMLInputElement;
    first.focus();

    first.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );

    expect(document.activeElement?.id).toBe('last');
  });
});
