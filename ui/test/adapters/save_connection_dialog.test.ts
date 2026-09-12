// @vitest-environment jsdom
// Role: test — the three small dialogs spec 26 adds: Save connection in both its shapes,
// Rename, and the question Forget asks (decisions 15, 18 and 19).

import { beforeEach, describe, expect, it } from 'vitest';

import {
  ForgetConnectionDialog,
  RenameConnectionDialog,
} from '../../src/adapters/rename_connection_dialog';
import {
  ALSO_LATER,
  NOT_AGAIN,
  SaveConnectionDialog,
  suggestion,
} from '../../src/adapters/save_connection_dialog';
import type { Connected } from '../../src/protocol';

const SKELETON = `
<dialog id="save-connection-dialog" aria-labelledby="save-connection-title"
        aria-describedby="save-why">
  <div role="application" aria-label="Save connection">
    <h1 id="save-connection-title">Save connection</h1>
    <p id="save-why"></p>
    <label for="save-name">Name</label>
    <input id="save-name" type="text" />
    <p id="save-also-later" hidden></p>
    <p hidden>
      <input id="save-not-again" type="checkbox" />
      <label for="save-not-again">Do not offer to save new connections</label>
    </p>
    <button id="save-ok" type="button">Save</button>
    <button id="save-cancel" type="button">Cancel</button>
  </div>
</dialog>
<dialog id="rename-connection-dialog" aria-labelledby="rename-connection-title">
  <div role="application" aria-label="Rename connection">
    <h1 id="rename-connection-title">Rename connection</h1>
    <label for="rename-name">Name</label>
    <input id="rename-name" type="text" />
    <button id="rename-ok" type="button">Rename</button>
    <button id="rename-cancel" type="button">Cancel</button>
  </div>
</dialog>
<dialog id="forget-connection-dialog" aria-label="Forget connection"
        aria-describedby="forget-why">
  <div role="application" aria-label="Forget connection">
    <h1>Forget connection</h1>
    <p id="forget-why"></p>
    <button id="forget-ok" type="button">Forget</button>
    <button id="forget-cancel" type="button">Cancel</button>
  </div>
</dialog>
<input id="command-input" />
`;

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

/** jsdom has no dialog implementation; these are the two parts these adapters use. */
function shim(id: string): void {
  const element = byId<HTMLDialogElement>(id);
  element.showModal = function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  element.close = function close(this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event('close'));
  };
}

function click(id: string): void {
  byId(id).dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

function connected(over: Partial<Connected> = {}): Connected {
  return {
    session: 1,
    label: 'SSH: marlon at example.org',
    note: null,
    limit_explained: false,
    saved_as: null,
    line_owner: 'FarEnd',
    ...over,
  };
}

let save: SaveConnectionDialog;
let rename: RenameConnectionDialog;
let forget: ForgetConnectionDialog;

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  for (const id of [
    'save-connection-dialog',
    'rename-connection-dialog',
    'forget-connection-dialog',
  ]) {
    shim(id);
  }
  save = new SaveConnectionDialog(
    byId<HTMLDialogElement>('save-connection-dialog'),
    byId('save-why'),
    byId<HTMLInputElement>('save-name'),
    byId('save-also-later'),
    byId<HTMLInputElement>('save-not-again'),
    byId<HTMLButtonElement>('save-ok'),
    byId<HTMLButtonElement>('save-cancel'),
  );
  rename = new RenameConnectionDialog(
    byId<HTMLDialogElement>('rename-connection-dialog'),
    byId<HTMLInputElement>('rename-name'),
    byId<HTMLButtonElement>('rename-ok'),
    byId<HTMLButtonElement>('rename-cancel'),
  );
  forget = new ForgetConnectionDialog(
    byId<HTMLDialogElement>('forget-connection-dialog'),
    byId('forget-why'),
    byId<HTMLButtonElement>('forget-ok'),
    byId<HTMLButtonElement>('forget-cancel'),
  );
});

describe('what the Save dialog suggests', () => {
  /**
   * **The session's origin when it has one** (decision 18): File → Save connection after
   * somebody changed a port offers the name they already know it by.
   */
  it('offers the name the session was started under', () => {
    expect(suggestion(connected({ saved_as: 'work laptop' }))).toBe('work laptop');
  });

  /**
   * **And otherwise a name they would have typed**, which is the connect list's own label
   * with the category taken off — exactly decision 18's list.
   */
  it('offers a name that says what the far end is', () => {
    expect(suggestion(connected())).toBe('marlon at example.org');
    expect(suggestion(connected({ label: 'WSL: Ubuntu' }))).toBe('Ubuntu');
    expect(suggestion(connected({ label: 'Scripted: builtin' }))).toBe('builtin');
    expect(suggestion(connected({ label: 'PowerShell 7' }))).toBe('PowerShell 7');
    expect(suggestion(connected({ label: 'Command Prompt' }))).toBe('Command Prompt');
  });
});

describe('the Save dialog', () => {
  /**
   * **Focus lands on the field with its text selected** (decision 18), so typing replaces
   * the suggestion and a listener who likes it presses Enter.
   */
  it('opens on the field, prefilled and selected', async () => {
    const asking = save.ask('work laptop', false);

    expect(document.activeElement?.id).toBe('save-name');
    expect(byId<HTMLInputElement>('save-name').value).toBe('work laptop');
    expect(byId<HTMLInputElement>('save-name').selectionStart).toBe(0);
    expect(byId<HTMLInputElement>('save-name').selectionEnd).toBe(
      'work laptop'.length,
    );

    click('save-cancel');
    await asking;
  });

  it('answers the name that was typed', async () => {
    const asking = save.ask('marlon at example.org', false);
    byId<HTMLInputElement>('save-name').value = 'work laptop';

    click('save-ok');

    await expect(asking).resolves.toEqual({
      name: 'work laptop',
      stopOffering: false,
    });
  });

  /** Enter from the field saves, because the whole dialog is one box and two buttons. */
  it('saves on Enter from the field', async () => {
    const asking = save.ask('work laptop', false);

    byId('save-name').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );

    await expect(asking).resolves.toEqual({
      name: 'work laptop',
      stopOffering: false,
    });
  });

  /** Escape, and every other way of closing, saves nothing. */
  it('answers nothing when it is closed', async () => {
    const asking = save.ask('work laptop', false);

    byId<HTMLDialogElement>('save-connection-dialog').close();

    await expect(asking).resolves.toEqual({ name: null, stopOffering: false });
  });

  /**
   * **The File-menu shape has neither the paragraph nor the checkbox** (decision 19): they
   * are about the *offer*, and there is no offer when the user asked.
   */
  it('hides the extras that belong to the offer when the user asked for it', async () => {
    const asking = save.ask('work laptop', false);

    expect(byId('save-also-later').hidden).toBe(true);
    expect(byId('save-not-again').closest('p')?.hidden).toBe(true);
    expect(byId('save-cancel').textContent).toBe('Cancel');

    click('save-cancel');
    await asking;
  });

  /**
   * **And the offering shape has both, with Not now in place of Cancel** — there is
   * nothing to cancel, and a listener who meets Cancel on an offer they did not ask for is
   * being told they interrupted something.
   */
  it('shows them, and says Not now, when Acter is the one asking', async () => {
    const asking = save.ask('marlon at example.org', true);

    expect(byId('save-also-later').hidden).toBe(false);
    expect(byId('save-also-later').textContent).toBe(ALSO_LATER);
    expect(byId('save-not-again').closest('p')?.hidden).toBe(false);
    expect(byId('save-cancel').textContent).toBe('Not now');

    click('save-cancel');
    await asking;
  });

  /**
   * **The checkbox is recorded whichever button they pressed** (decision 19): ticking it
   * is a decision about the offer rather than about this connection.
   */
  it('reports the checkbox whether they saved or said not now', async () => {
    const saying = save.ask('one', true);
    byId<HTMLInputElement>('save-not-again').checked = true;
    click('save-cancel');
    await expect(saying).resolves.toEqual({ name: null, stopOffering: true });

    const saving = save.ask('two', true);
    byId<HTMLInputElement>('save-not-again').checked = true;
    click('save-ok');
    await expect(saving).resolves.toEqual({ name: 'two', stopOffering: true });
  });

  /** And it starts unticked every time, so nothing is carried over from last time. */
  it('starts with the box unticked', async () => {
    const first = save.ask('one', true);
    byId<HTMLInputElement>('save-not-again').checked = true;
    click('save-cancel');
    await first;

    const second = save.ask('two', true);

    expect(byId<HTMLInputElement>('save-not-again').checked).toBe(false);
    click('save-cancel');
    await second;
  });

  /**
   * **A refusal keeps the dialog open** with focus back in the field (decision 18), which
   * is where trying something else begins.
   */
  it('stays open and puts focus back in the field when it is refused', async () => {
    const asking = save.ask('work laptop', false);
    click('save-ok');
    await asking;

    save.refused();

    expect(byId<HTMLDialogElement>('save-connection-dialog').open).toBe(true);
    expect(document.activeElement?.id).toBe('save-name');
  });

  /** Both sentences it can carry are whole ones a reader can speak. */
  it('says why it is there, as a sentence', async () => {
    const asked = save.ask('work laptop', false);
    const forTheMenu = byId('save-why').textContent ?? '';
    click('save-cancel');
    await asked;

    const offered = save.ask('work laptop', true);
    const forTheOffer = byId('save-why').textContent ?? '';
    click('save-cancel');
    await offered;

    for (const said of [forTheMenu, forTheOffer, ALSO_LATER, NOT_AGAIN]) {
      expect(said.trim()).not.toBe('');
      expect(said).not.toContain('  ');
    }
    expect(forTheMenu.endsWith('.')).toBe(true);
    expect(forTheOffer.endsWith('.')).toBe(true);
    expect(forTheMenu).not.toBe(forTheOffer);
  });
});

describe('the Rename dialog', () => {
  it('opens on the field with the old name prefilled and selected', async () => {
    const asking = rename.ask('work laptop');

    expect(document.activeElement?.id).toBe('rename-name');
    expect(byId<HTMLInputElement>('rename-name').value).toBe('work laptop');
    expect(byId<HTMLInputElement>('rename-name').selectionEnd).toBe(
      'work laptop'.length,
    );

    click('rename-cancel');
    await asking;
  });

  it('answers the new name, and nothing when it is cancelled', async () => {
    const renaming = rename.ask('work laptop');
    byId<HTMLInputElement>('rename-name').value = 'home';
    click('rename-ok');
    await expect(renaming).resolves.toBe('home');

    const cancelling = rename.ask('home');
    click('rename-cancel');
    await expect(cancelling).resolves.toBe(null);
  });

  it('renames on Enter from the field', async () => {
    const renaming = rename.ask('work laptop');
    byId<HTMLInputElement>('rename-name').value = 'home';

    byId('rename-name').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );

    await expect(renaming).resolves.toBe('home');
  });
});

describe('the Forget dialog', () => {
  /**
   * **Focus opens on Cancel**, so the answer Enter gives is the one that keeps the
   * connection: this is the one action here nobody can undo.
   */
  it('opens on Cancel, with the question as its description', async () => {
    const asking = forget.ask('Forget Ada? Nothing else changes.');

    expect(byId('forget-why').textContent).toBe('Forget Ada? Nothing else changes.');
    expect(document.activeElement?.id).toBe('forget-cancel');

    click('forget-cancel');
    await asking;
  });

  it('answers yes only when Forget is pressed', async () => {
    const forgetting = forget.ask('Forget Ada?');
    click('forget-ok');
    await expect(forgetting).resolves.toBe(true);

    const keeping = forget.ask('Forget Ada?');
    click('forget-cancel');
    await expect(keeping).resolves.toBe(false);
  });

  /** Escape, and every other way of closing, keeps the connection. */
  it('keeps the connection when it is closed any other way', async () => {
    const asking = forget.ask('Forget Ada?');

    byId<HTMLDialogElement>('forget-connection-dialog').close();

    await expect(asking).resolves.toBe(false);
  });
});
