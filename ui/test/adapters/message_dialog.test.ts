// @vitest-environment jsdom
// Role: test — the modal that says one thing and waits to be dismissed.
//
// It exists because a connection error used to arrive as a live-region announcement and a
// user expected something to press OK on (reported 2026-08-26). What is pinned here is the
// two properties that make it different from an announcement: the sentence is the dialog's
// own description, so a reader says it on opening, and the promise does not settle until
// somebody has dismissed it.

import { beforeEach, describe, expect, it } from 'vitest';

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
