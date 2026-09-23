// @vitest-environment jsdom
// Role: test — the modal that says one thing and waits to be dismissed.

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

    expect(tab.defaultPrevented).toBe(true);
    expect(focused).toBe(0);
    expect(document.activeElement?.id).toBe('failed-ok');

    dialog.close('ok');
  });
});

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
