// @vitest-environment jsdom
// Role: test — the Connect dialog's behaviour: what it lists, what the panel holds, what
// it announces when the kind changes, and what it does with the two answers connecting can
// give (spec A8).

import { beforeEach, describe, expect, it } from 'vitest';

import { ConnectDialog, panelSummary } from '../../src/adapters/connect_dialog';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { ConnectApi } from '../../src/ports/connect_api';
import type { Connectable, Connected, ProfileId } from '../../src/protocol';

// The dialog's static skeleton, copied from views/main_window.html. It is a copy on
// purpose: what this file asserts is the behaviour over that structure, and the structure
// itself is what the E2E spec and the NVDA pass drive in the real document.
const SKELETON = `
<dialog id="connect-dialog" aria-labelledby="connect-title">
  <h1 id="connect-title">Connect</h1>
  <div role="application" aria-label="Connection kind">
    <ul id="connect-kinds" role="listbox" aria-label="Connection kind" tabindex="0"></ul>
  </div>
  <div id="connect-panel" role="group" aria-labelledby="connect-panel-title" tabindex="-1">
    <h2 id="connect-panel-title">Options</h2>
    <div id="connect-panel-body"></div>
  </div>
  <button id="connect-start" type="button">Connect</button>
  <button id="connect-cancel" type="button">Cancel</button>
</dialog>
<input id="command-input" />
`;

function cmd(): Connectable {
  return {
    id: { profile: 'Shell', kind: 'Cmd' },
    label: 'Command Prompt',
    available: true,
    instructions: null,
    variants: [],
  };
}

function wsl(): Connectable {
  return {
    id: { profile: 'Shell', kind: 'Wsl' },
    label: 'WSL',
    available: true,
    instructions: null,
    variants: [
      { id: { profile: 'Distribution', name: 'Ubuntu' }, label: 'Ubuntu' },
      { id: { profile: 'Distribution', name: 'Debian' }, label: 'Debian' },
    ],
  };
}

function missing(): Connectable {
  return {
    id: { profile: 'Shell', kind: 'PowerShellSeven' },
    label: 'PowerShell 7 (not available)',
    available: false,
    instructions:
      'PowerShell 7 is not installed. Install it by running winget install Microsoft.PowerShell from any terminal.',
    variants: [],
  };
}

class FakeConnect implements ConnectApi {
  rows: Connectable[] = [cmd(), wsl(), missing()];
  /** How many times the list was asked for, so "fresh every time" is assertable. */
  asked = 0;
  connectable(): Promise<Connectable[]> {
    this.asked += 1;
    return Promise.resolve(this.rows);
  }
  use(): Promise<Connected> {
    throw new Error('the dialog connects through its action, not through this port');
  }
  connected(): Promise<Connected | null> {
    return Promise.resolve(null);
  }
}

class FakeAnnouncer implements AnnouncerView {
  announcements: string[] = [];
  announce(text: string): void {
    this.announcements.push(text);
  }
}

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

let connect: FakeConnect;
let announcer: FakeAnnouncer;
let attempted: ProfileId[];
/** What the next connect attempt answers: connected, or could not be started. */
let succeeds: boolean;
let returned: number;
let dialog: ConnectDialog;

function make(): ConnectDialog {
  return new ConnectDialog(
    byId<HTMLDialogElement>('connect-dialog'),
    byId('connect-kinds'),
    byId('connect-panel-title'),
    byId('connect-panel-body'),
    connect,
    (id) => {
      attempted.push(id);
      return Promise.resolve(succeeds);
    },
    announcer,
    {
      focus: () => {
        returned += 1;
      },
    },
  );
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  // jsdom has no dialog implementation; these are the two parts this adapter uses.
  const element = byId<HTMLDialogElement>('connect-dialog');
  element.showModal = function showModal(this: HTMLDialogElement) {
    this.open = true;
  };
  element.close = function close(this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event('close'));
  };
  connect = new FakeConnect();
  announcer = new FakeAnnouncer();
  attempted = [];
  succeeds = true;
  returned = 0;
  dialog = make();
});

function options(): string[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>('#connect-kinds [role="option"]'),
  ).map((option) => option.textContent ?? '');
}

function selected(): string | undefined {
  return (
    document.querySelector<HTMLElement>('[role="option"][aria-selected="true"]')
      ?.textContent ?? undefined
  );
}

function press(key: string): void {
  byId('connect-kinds').dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true }),
  );
}

describe('opening', () => {
  it('lists the kinds the backend answered, in its order', async () => {
    await dialog.open();

    expect(options()).toEqual([
      'Command Prompt',
      'WSL',
      'PowerShell 7 (not available)',
    ]);
    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(true);
  });

  /** Decision 3: a distribution installed while Acter is running appears the next time the
   * dialog opens, with no restart — so the list is asked for again every time. */
  it('asks the machine again on every open', async () => {
    await dialog.open();
    byId<HTMLDialogElement>('connect-dialog').close();
    await dialog.open();

    expect(connect.asked).toBe(2);
  });

  /** Opening an open dialog throws InvalidStateError, and throws it into a `void` call
   * where nobody sees it. A menu item chosen twice is an ordinary thing. */
  it('is not broken by being opened twice', async () => {
    await dialog.open();
    await dialog.open();

    expect(connect.asked).toBe(1);
  });

  /** **Silent on arrival when the panel is empty**, and that is a measurement rather than
   * a preference: the reader reads a dialog as it opens, including a live region inside it
   * that already has text, so an unconditional announcement here is heard twice. Nothing is
   * hidden — an empty panel is not a change a listener has to be told about on arrival. */
  it('says nothing about a panel that is empty when it opens', async () => {
    await dialog.open();

    expect(announcer.announcements).toEqual([]);
  });

  /** But a panel that already has something in it is announced, because arriving to find a
   * second control there unmentioned is the same trap as one appearing silently. */
  it('says what the panel holds when it opens on a kind that has one', async () => {
    connect.rows = [wsl(), cmd()];

    await dialog.open();

    expect(announcer.announcements).toEqual(['2 distributions']);
  });

  /** The listbox names the kind itself, with its position, so the option carries the name
   * and this module never repeats it. */
  it('names the selected option to the reader from the first render', async () => {
    await dialog.open();

    expect(byId('connect-kinds').getAttribute('aria-activedescendant')).toBe(
      'connect-kind-0',
    );
    expect(selected()).toBe('Command Prompt');
  });
});

describe('the panel (decision 2)', () => {
  it('holds nothing for a kind that needs nothing', async () => {
    await dialog.open();

    expect(byId('connect-panel-title').textContent).toBe('no options');
    expect(byId('connect-panel-body').children).toHaveLength(0);
  });

  it('holds the distributions for WSL, named without repeating the kind', async () => {
    await dialog.open();
    press('ArrowDown');

    const select = byId<HTMLSelectElement>('connect-variant');
    expect(Array.from(select.options).map((option) => option.textContent)).toEqual([
      'Ubuntu',
      'Debian',
    ]);
    expect(byId('connect-panel-title').textContent).toBe('2 distributions');
  });

  /** B5.4's whole argument, rendered: a kind this machine cannot start is still in the
   * list, still readable, and the panel says what to do about it. */
  it('holds what to do about a kind this machine cannot start', async () => {
    await dialog.open();
    press('End');

    expect(byId('connect-panel-title').textContent).toBe('not available');
    expect(byId('connect-panel-body').textContent).toContain('winget install');
  });

  /** **The whole of the old submenu objection, answered.** A second control that changes
   * silently behind a listener arrowing a list is the classic non-visual trap. */
  it('announces what it now holds every time the kind changes', async () => {
    await dialog.open();
    announcer.announcements = [];

    press('ArrowDown');
    press('ArrowDown');

    // The kind itself is not repeated: the listbox has already announced it with its
    // position, and saying it again made a listener hear it twice for one arrow press.
    expect(announcer.announcements).toEqual(['2 distributions', 'not available']);
  });

  it('counts one distribution without pluralising it', () => {
    const one = wsl();
    one.variants = one.variants.slice(0, 1);

    expect(panelSummary(one)).toBe('1 distribution');
  });
});

describe('arrowing the kinds', () => {
  /** A list you cannot arrow through without leaving it is not a list, so selection moves
   * and focus does not (decision 2). */
  it('moves the selection and leaves focus on the list', async () => {
    await dialog.open();

    press('ArrowDown');

    expect(selected()).toBe('WSL');
    expect(byId('connect-kinds').getAttribute('aria-activedescendant')).toBe(
      'connect-kind-1',
    );
    expect(document.activeElement).toBe(byId('connect-kinds'));
  });

  it('stops at both ends rather than wrapping', async () => {
    await dialog.open();

    press('ArrowUp');
    expect(selected()).toBe('Command Prompt');

    press('End');
    press('ArrowDown');
    expect(selected()).toBe('PowerShell 7 (not available)');
  });

  it('home and end reach the first and last kinds', async () => {
    await dialog.open();
    press('End');
    expect(selected()).toBe('PowerShell 7 (not available)');

    press('Home');
    expect(selected()).toBe('Command Prompt');
  });
});

describe('connecting (decision 4)', () => {
  it('starts the chosen kind and closes on success', async () => {
    await dialog.open();

    byId('connect-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'Cmd' }]);
    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(false);
    expect(returned).toBe(1);
  });

  it('starts the chosen distribution rather than the kind when the panel offered any', async () => {
    await dialog.open();
    press('ArrowDown');
    byId<HTMLSelectElement>('connect-variant').value = '1';

    byId('connect-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Distribution', name: 'Debian' }]);
  });

  it('enter on the list connects, the way enter on a chosen row should', async () => {
    await dialog.open();

    press('Enter');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'Cmd' }]);
  });

  /** **Where a dialog beats a submenu a second time.** A submenu that failed had nowhere
   * to put the user back; this leaves them on the list, able to choose something else. */
  it('stays open when the connection could not be started', async () => {
    succeeds = false;
    await dialog.open();

    byId('connect-start').click();
    await Promise.resolve();

    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(true);
    expect(returned).toBe(0);
  });

  /** A kind this machine cannot start is not special-cased here: the call goes through and
   * the backend refuses it with the instructions the panel is already showing. One path,
   * and no disabled control that reads differently from how it looks. */
  it('still attempts a kind the machine cannot start, and lets the backend refuse it', async () => {
    succeeds = false;
    await dialog.open();
    press('End');

    byId('connect-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(true);
  });
});

describe('leaving', () => {
  it('cancel closes it and puts focus back in the edit field', async () => {
    await dialog.open();

    byId('connect-cancel').click();

    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(false);
    expect(returned).toBe(1);
    expect(attempted).toEqual([]);
  });

  /** Escape is the platform's own and closes the dialog itself; what is not the
   * platform's is where focus belongs afterwards. */
  it('closing by any route returns focus to the edit field', async () => {
    await dialog.open();

    byId<HTMLDialogElement>('connect-dialog').close();

    expect(returned).toBe(1);
  });
});

// **The platform does not cycle Tab for a modal dialog** — Chromium sends focus from the
// last control to the dialog's own document, which NVDA met twice: once in the About dialog
// in 2026-08-24's pass, and again here on 2026-08-26, where Tab past Cancel announced
// "dialog Connect" and left the reader nowhere.
describe('keeping Tab inside', () => {
  it('cycles from the last control back to the first', async () => {
    await dialog.open();
    byId('connect-cancel').focus();

    press2('Tab');

    expect(document.activeElement).toBe(byId('connect-kinds'));
  });

  it('cycles backwards from the first control to the last', async () => {
    await dialog.open();
    byId('connect-kinds').focus();

    press2('Tab', true);

    expect(document.activeElement).toBe(byId('connect-cancel'));
  });

  it('walks forwards through every control in order', async () => {
    await dialog.open();
    press('ArrowDown'); // WSL, so the panel has a combo box in the tab order
    byId('connect-kinds').focus();

    const walked: string[] = [];
    for (let step = 0; step < 4; step += 1) {
      press2('Tab');
      walked.push(document.activeElement?.id ?? '');
    }

    expect(walked).toEqual([
      'connect-variant',
      'connect-start',
      'connect-cancel',
      'connect-kinds',
    ]);
  });
});

/** A Tab pressed on the dialog, which is where the trap listens. */
function press2(key: string, shift = false): void {
  byId('connect-dialog').dispatchEvent(
    new KeyboardEvent('keydown', { key, shiftKey: shift, bubbles: true }),
  );
}
