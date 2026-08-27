// @vitest-environment jsdom
// Role: test — the credential dialog, and the two properties that make it worth having:
// the field is masked, and the value does not stay in the document afterwards.

import { beforeEach, describe, expect, it } from 'vitest';

import { PasswordDialog } from '../../src/adapters/password_dialog';
import type { ConnectQuestion } from '../../src/protocol';

type Password = Extract<ConnectQuestion, { question: 'Password' }>;

const ASKING: Password = {
  question: 'Password',
  host: 'acter-ssh',
  user: 'acter',
  again: false,
};

function build(): {
  dialog: HTMLDialogElement;
  field: HTMLInputElement;
  ask: PasswordDialog;
} {
  document.body.innerHTML = `
    <dialog id="password-dialog">
      <p id="password-prompt"></p>
      <form method="dialog">
        <label for="password-field">Password</label>
        <input id="password-field" type="password" autocomplete="off" />
        <button type="submit" value="submit">Sign in</button>
        <button type="submit" value="cancel">Cancel</button>
      </form>
    </dialog>`;
  const dialog = document.getElementById('password-dialog') as HTMLDialogElement;
  // jsdom implements `<dialog>` only partially depending on version; these keep the suite
  // about Acter's behaviour rather than about jsdom's coverage of the element. `close`
  // carries the value the way the platform does, because the returned value is exactly what
  // this adapter reads the decision from.
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
  const field = document.getElementById('password-field') as HTMLInputElement;
  return {
    dialog,
    field,
    ask: new PasswordDialog(
      dialog,
      document.getElementById('password-prompt') as HTMLElement,
      field,
    ),
  };
}

describe('PasswordDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  // A listener answering two connections, or one that reappeared, is otherwise typing a
  // password into a dialog that could belong to anything.
  it('says which account on which host is being signed in to', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(ASKING);

    expect(document.getElementById('password-prompt')?.textContent).toContain(
      'acter at acter-ssh',
    );

    dialog.close('cancel');
    await answered;
  });

  // **A second prompt with no explanation is indistinguishable from the first one not
  // having been submitted**, which is precisely the confusion this product exists to remove.
  it('says so when a password was already tried and refused', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask({ ...ASKING, again: true });

    expect(document.getElementById('password-prompt')?.textContent).toContain(
      'not accepted',
    );

    dialog.close('cancel');
    await answered;
  });

  it('is a masked field, and focus starts in it', async () => {
    const { dialog, field, ask } = build();

    const answered = ask.ask(ASKING);

    expect(field.type).toBe('password');
    expect(document.activeElement).toBe(field);

    dialog.close('cancel');
    await answered;
  });

  it('hands over what was typed when it was submitted', async () => {
    const { dialog, field, ask } = build();

    const answered = ask.ask(ASKING);
    field.value = 'hunter2';
    dialog.close('submit');

    await expect(answered).resolves.toEqual({
      answer: 'Password',
      secret: 'hunter2',
    });
  });

  // **Nothing is left in the document**, so a password is not sitting in a window that
  // stays open for hours, or in a screenshot taken later.
  it('clears the field once the value has been handed over', async () => {
    const { dialog, field, ask } = build();

    const answered = ask.ask(ASKING);
    field.value = 'hunter2';
    dialog.close('submit');
    await answered;

    expect(field.value).toBe('');
  });

  // Not giving one is a decision rather than a failure, and every way out except the submit
  // button means it.
  it.each(['cancel', ''])('gives up when closed with %o', async (value) => {
    const { dialog, field, ask } = build();

    const answered = ask.ask(ASKING);
    field.value = 'typed but not submitted';
    dialog.close(value);

    await expect(answered).resolves.toEqual({ answer: 'GiveUp' });
    expect(field.value).toBe('');
  });
});
