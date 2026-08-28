// @vitest-environment jsdom
// Role: test — the dialog that asks whether to start a program Windows would not vouch for.
//
// What is asserted here is mostly about **not starting being the default**, because that is
// the one property a mistake in this file would quietly take away — and about the file path
// being walkable, because a path nobody can read character by character is a path nobody can
// tell from the one they expected (spec B5.7, decision 6).

import { beforeEach, describe, expect, it } from 'vitest';

import { UnverifiedDialog } from '../../src/adapters/unverified_dialog';
import type { ConnectQuestion } from '../../src/protocol';

type Unverified = Extract<ConnectQuestion, { question: 'Unverified' }>;

const UNSIGNED: Unverified = {
  question: 'Unverified',
  label: 'PowerShell 7',
  program: 'C:\\tools\\pwsh\\pwsh.exe',
  said: 'Nothing has signed this file, so there is no record of who built it or whether it has been changed since. Start it only if you know how it got there.',
  signer: null,
};

const SOMEBODY_ELSE: Unverified = {
  ...UNSIGNED,
  said: 'This file is signed by Contoso Corporation, and this computer does not trust whoever issued that signature. Start it only if you know why that certificate is not trusted here.',
  signer: 'Contoso Corporation',
};

function build(): { dialog: HTMLDialogElement; ask: UnverifiedDialog } {
  document.body.innerHTML = `
    <dialog id="unverified-dialog">
      <h1>Acter could not confirm who made this program</h1>
      <p id="unverified-summary"></p>
      <div id="unverified-body"></div>
      <form method="dialog">
        <button id="unverified-refuse" type="submit" value="refuse">Do not start it</button>
        <button id="unverified-start" type="submit" value="start">Start it anyway</button>
      </form>
    </dialog>`;
  const dialog = document.getElementById('unverified-dialog') as HTMLDialogElement;
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
  return {
    dialog,
    ask: new UnverifiedDialog(
      dialog,
      document.getElementById('unverified-summary') as HTMLElement,
      document.getElementById('unverified-body') as HTMLElement,
    ),
  };
}

describe('UnverifiedDialog', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('says what was chosen and what was found, as the dialog opens', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNSIGNED);
    const summary = document.getElementById('unverified-summary')?.textContent ?? '';

    // **Spoken as the dialog opens**, via aria-describedby: what they chose, what was found,
    // and what to do about it. The sentence is the backend's own — the words are decided in
    // one place, in the domain, and this renders them.
    expect(summary).toContain('PowerShell 7');
    expect(summary).toContain('Nothing has signed this file');
    expect(summary).toContain('Start it only if');

    dialog.close('refuse');
    await answered;
  });

  it('shows the full path as a field that can be walked and cannot be changed', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNSIGNED);
    const shown = document.getElementById('unverified-program') as HTMLInputElement;

    // **The full path, in a text box.** The thing this check defeats is somebody putting a
    // different file where a name used to point, so which directory it is in is the whole of
    // what makes it recognisably wrong — and inside `role="application"` a paragraph cannot
    // be arrowed at all.
    expect(shown.tagName).toBe('INPUT');
    expect(shown.value).toBe(UNSIGNED.program);
    expect(shown.readOnly).toBe(false);

    // Editable so the caret is real, and every edit refused so the value cannot change.
    const refused = new InputEvent('beforeinput', { cancelable: true, bubbles: true });
    shown.dispatchEvent(refused);
    expect(refused.defaultPrevented).toBe(true);

    dialog.close('refuse');
    await answered;
  });

  it('names who signed it when anybody did, and says nothing when nobody did', async () => {
    const first = build();
    const asked = first.ask.ask(SOMEBODY_ELSE);
    const signer = document.getElementById('unverified-signer') as HTMLInputElement;

    expect(signer.value).toBe('Contoso Corporation');
    first.dialog.close('refuse');
    await asked;

    const second = build();
    const again = second.ask.ask(UNSIGNED);

    // Nothing signed it, so there is nobody to name — and an empty box labelled "Signed by"
    // is a tab stop that answers a question with silence.
    expect(document.getElementById('unverified-signer')).toBeNull();
    second.dialog.close('refuse');
    await again;
  });

  it('starts the program only when the button that says so is pressed', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNSIGNED);
    dialog.close('start');

    await expect(answered).resolves.toEqual({ answer: 'StartAnyway' });
  });

  it('answers do-not-start for every other way out of the dialog', async () => {
    // **The default is not to start, and every way of leaving reaches it**: the refusing
    // button, Escape, and the dialog being closed by anything else at all. This is the one
    // property a mistake here would quietly take away.
    for (const how of ['refuse', undefined, 'anything else']) {
      const { dialog, ask } = build();

      const answered = ask.ask(UNSIGNED);
      dialog.close(how);

      await expect(answered).resolves.toEqual({ answer: 'GiveUp' });
    }
  });

  it('has no default action, so Enter outside a button does nothing', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNSIGNED);
    const shown = document.getElementById('unverified-program') as HTMLElement;
    const pressed = new KeyboardEvent('keydown', {
      key: 'Enter',
      cancelable: true,
      bubbles: true,
    });
    shown.dispatchEvent(pressed);

    // Neither outcome is reachable by the key people press without thinking; going to a
    // button is itself the deliberate act.
    expect(pressed.defaultPrevented).toBe(true);
    expect(dialog.open).toBe(true);

    const onButton = new KeyboardEvent('keydown', {
      key: 'Enter',
      cancelable: true,
      bubbles: true,
    });
    document.getElementById('unverified-start')?.dispatchEvent(onButton);
    expect(onButton.defaultPrevented).toBe(false);

    dialog.close('refuse');
    await answered;
  });

  it('opens on the path, which is the thing the dialog is for', async () => {
    const { dialog, ask } = build();

    const answered = ask.ask(UNSIGNED);

    expect(document.activeElement?.id).toBe('unverified-program');

    dialog.close('refuse');
    await answered;
  });
});
