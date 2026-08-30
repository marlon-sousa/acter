// @vitest-environment jsdom
// Role: test — the Connect dialog's behaviour: what it lists, what the panel holds, what
// it announces when the kind changes, and what it does with the two answers connecting can
// give (spec A8).

import { beforeEach, describe, expect, it } from 'vitest';

import { ConnectDialog, panelSummary } from '../../src/adapters/connect_dialog';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { ConnectApi } from '../../src/ports/connect_api';
import type { HelpView } from '../../src/ports/help_view';
import type {
  Connectable,
  Connected,
  ProfileId,
  SetUp,
} from '../../src/protocol';

// The dialog's static skeleton, copied from views/main_window.html. It is a copy on
// purpose: what this file asserts is the behaviour over that structure, and the structure
// itself is what the E2E spec and the NVDA pass drive in the real document.
const SKELETON = `
<dialog id="connect-dialog" aria-labelledby="connect-title">
  <h1 id="connect-title">Connect</h1>
  <div role="application" aria-label="Connect">
    <ul id="connect-kinds" role="listbox" aria-label="Connection kind" tabindex="0"></ul>
    <div id="connect-panel" role="group" aria-labelledby="connect-panel-title" tabindex="-1">
      <h2 id="connect-panel-title">Options</h2>
      <div id="connect-panel-body"></div>
    </div>
    <p>
      <input id="connect-set-up" type="checkbox" checked />
      <label for="connect-set-up"
        >Let Acter set this session up so it can tell you more about what you run</label
      >
    </p>
    <button id="connect-set-up-help" type="button">Help with setting a session up</button>
    <button id="connect-start" type="button">Connect</button>
    <button id="connect-cancel" type="button">Cancel</button>
  </div>
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
      {
        id: { profile: 'Distribution', name: 'Ubuntu' },
        label: 'Ubuntu',
        available: true,
        instructions: null,
      },
      {
        id: { profile: 'Distribution', name: 'Debian' },
        label: 'Debian',
        available: true,
        instructions: null,
      },
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

/** PowerShell as A11 shapes it: one kind, its editions as variants, one of them missing. */
function powershell(): Connectable {
  return {
    id: { profile: 'Shell', kind: 'WindowsPowerShell' },
    label: 'PowerShell',
    available: true,
    instructions: null,
    variants: [
      {
        id: { profile: 'Shell', kind: 'WindowsPowerShell' },
        label: 'Windows PowerShell',
        available: true,
        instructions: null,
      },
      {
        id: { profile: 'Shell', kind: 'PowerShellSeven' },
        label: 'PowerShell 7 (not available)',
        available: false,
        instructions:
          'PowerShell 7 is not installed. Install it by running winget install Microsoft.PowerShell from any terminal.',
      },
    ],
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

/** The dialog Enter goes to while a connection is being made (reported 2026-08-30). */
class FakeConnecting {
  /** What it was told is being connected to, once per attempt. */
  shown: string[] = [];
  hidden = 0;
  show(label: string): void {
    this.shown.push(label);
  }
  hide(): void {
    this.hidden += 1;
  }
}

/** Where the Help button beside the set-up checkbox leads. */
class FakeHelp implements HelpView {
  opened: { topic?: string; returnTo?: { focus(): void } }[] = [];
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void {
    this.opened.push(options ?? {});
  }
}

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

/**
 * Choose one of the panel's variants, the way a person does: the value changes and the
 * control says so. Since 2026-08-30 a list starts on nothing chosen, so setting the value
 * without the event leaves the dialog believing — correctly — that nobody has chosen.
 */
function chooseVariant(value: string): void {
  const select = byId<HTMLSelectElement>('connect-variant');
  select.value = value;
  select.dispatchEvent(new Event('change'));
}

let connect: FakeConnect;
let announcer: FakeAnnouncer;
let attempted: ProfileId[];
/** What the checkbox said on each attempt (spec B9.5, decision 9). */
let asked: SetUp[];
/** What the next connect attempt answers: connected, or could not be started. */
let succeeds: boolean;
let returned: number;
let connecting: FakeConnecting;
let help: FakeHelp;
let dialog: ConnectDialog;

function make(): ConnectDialog {
  return new ConnectDialog(
    byId<HTMLDialogElement>('connect-dialog'),
    byId('connect-kinds'),
    byId('connect-panel-title'),
    byId('connect-panel-body'),
    connect,
    (id, setUp) => {
      attempted.push(id);
      asked.push(setUp);
      return Promise.resolve(succeeds);
    },
    announcer,
    {
      focus: () => {
        returned += 1;
      },
    },
    connecting,
    help,
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
  connecting = new FakeConnecting();
  help = new FakeHelp();
  attempted = [];
  asked = [];
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
  // **A8 decision 2, reversed on use 2026-08-26.** The panel no longer describes itself
  // when it holds something ordinary; what survives is a row that cannot be started.
  it('says nothing when it opens on a kind that can be started', async () => {
    await dialog.open();

    expect(announcer.announcements).toEqual([]);
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
      'not chosen',
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
  // **Reversed on use, 2026-08-26**, reported by the user driving the real dialog:
  // arrowing between kinds that can be started says nothing about the panel. A second
  // utterance between every arrow press is paid on every navigation for a benefit that
  // lands occasionally, and "no options" is a sentence about an empty container.
  //
  // What survives is the row that cannot be started at all, which is a fact rather than a
  // description of the panel.
  it('says nothing about a panel holding an ordinary choice, and speaks for one that cannot be started', async () => {
    await dialog.open();
    announcer.announcements = [];

    press('ArrowDown');
    press('ArrowDown');

    expect(announcer.announcements).toEqual(['not available']);
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
    chooseVariant('1');

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
    // And a distribution chosen, so Connect is available: a disabled control is not a tab
    // stop, which is what this walk is about.
    chooseVariant('1');
    byId('connect-kinds').focus();

    const walked: string[] = [];
    for (let step = 0; step < 6; step += 1) {
      press2('Tab');
      walked.push(document.activeElement?.id ?? '');
    }

    // The checkbox sits between the panel and the buttons, which is the order somebody works
    // through this dialog in: choose a kind, fill in what it needs, decide whether the
    // session is set up, then connect (spec B9.5, decision 9).
    expect(walked).toEqual([
      'connect-variant',
      'connect-set-up',
      // What the box above it means, next to the box (reported 2026-08-30).
      'connect-set-up-help',
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

// **Enter is the dialog's default action, from anywhere in it.** It was handled on the
// kinds list alone, so a user who tabbed into the panel, chose a distribution and pressed
// Enter got nothing at all — reported by the user on 2026-08-26, choosing Debian.
describe('Enter as the default action', () => {
  function enterOn(id: string): void {
    byId(id).dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
  }

  it('connects from the distribution combo box', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant('1');

    enterOn('connect-variant');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Distribution', name: 'Debian' }]);
  });

  it('connects from the panel', async () => {
    await dialog.open();
    press('End');

    enterOn('connect-panel');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
  });

  /** A button answers Enter itself, and answering it here as well would connect when the
   * user pressed Cancel. */
  it('leaves a button to answer its own Enter', async () => {
    await dialog.open();

    enterOn('connect-cancel');
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });
});

// **PowerShell is one kind with its editions as variants** (spec A11), the shape WSL
// already had. What makes it different from WSL is that an edition can be *missing* while
// the kind is not — so B5.4's rule applies one level down: it stays in the panel and says
// what to do about it.
describe('a kind whose variants can be missing', () => {
  beforeEach(() => {
    connect.rows = [powershell(), cmd()];
  });

  it('names the panel after what its variants are', async () => {
    await dialog.open();

    expect(byId('connect-panel-title').textContent).toBe('2 editions');
    expect(document.querySelector('label[for="connect-variant"]')?.textContent).toBe(
      'Edition',
    );
  });

  it('lists the missing edition, saying so in its name', async () => {
    await dialog.open();

    const select = byId<HTMLSelectElement>('connect-variant');
    expect(Array.from(select.options).map((option) => option.textContent)).toEqual([
      'not chosen',
      'Windows PowerShell',
      'PowerShell 7 (not available)',
    ]);
  });

  /** Nothing to say about the edition that works, so the panel holds only the control. */
  it('shows no instructions while an available edition is chosen', async () => {
    await dialog.open();

    expect(byId('connect-panel-body').querySelector('[data-instructions]')).toBeNull();
  });

  /** And the panel must not change in silence: choosing the missing one puts what to do
   * about it in front of the listener, and says that it is there. */
  it('shows and announces what to do when the missing edition is chosen', async () => {
    await dialog.open();
    announcer.announcements = [];
    const select = byId<HTMLSelectElement>('connect-variant');

    select.value = '1';
    select.dispatchEvent(new Event('change'));

    const said = byId('connect-panel-body').querySelector('[data-instructions]');
    expect(said?.textContent).toContain('winget install');
    // Focusable, because prose inside an application region cannot be arrowed.
    expect((said as HTMLElement).tabIndex).toBe(0);
    expect(announcer.announcements).toEqual(['not available']);
  });

  /** Choosing it anyway goes through: the backend refuses it with the very words the panel
   * is showing, which is one path and one place the sentence is decided. */
  it('still attempts the missing edition, and lets the backend refuse it', async () => {
    succeeds = false;
    await dialog.open();
    const select = byId<HTMLSelectElement>('connect-variant');
    select.value = '1';
    select.dispatchEvent(new Event('change'));

    byId('connect-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(true);
  });
});

/**
 * SSH as the connect dialog holds it: the one kind that is a **form** rather than a choice.
 *
 * A submenu is the right shape for a pure choice and connecting to cmd is one; a host, a
 * port and an account are not (spec A8, decision 1). These pin the two things that would
 * otherwise go wrong quietly — that the panel says what it now holds, and that what the
 * Connect button starts is what was typed rather than the empty row.
 */
describe('the kind that is a form', () => {
  function ssh(): Connectable {
    return {
      // The row names no machine: what to connect to comes from the panel.
      id: { profile: 'Ssh', host: '', port: 22, user: '' },
      label: 'SSH',
      available: true,
      instructions: null,
      variants: [],
    };
  }

  beforeEach(() => {
    connect.rows = [cmd(), ssh()];
  });

  /** Type into one of the form's fields, the way a keyboard does. */
  function fill(name: string, value: string): void {
    const field = byId<HTMLInputElement>(`connect-ssh-${name}`);
    field.value = value;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  }

  // **Counting the boxes was the thing that made no sense** (reported 2026-08-26): it is
  // how much typing there is, not what there is to choose between, and it aped the variant
  // count while meaning something else.
  it('says nothing about the panel, and does not count the boxes', async () => {
    await dialog.open();
    announcer.announcements.length = 0;

    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(announcer.announcements).toEqual([]);
    expect(byId('connect-panel-title').textContent).toBe('Connection details');
  });

  it('offers a host, a port and an account, with the usual port filled in', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(byId<HTMLInputElement>('connect-ssh-host').value).toBe('');
    expect(byId<HTMLInputElement>('connect-ssh-port').value).toBe('22');
    expect(byId<HTMLInputElement>('connect-ssh-user').value).toBe('');
    // Labelled, because an unlabelled text box announces as "edit" and nothing else.
    expect(document.querySelector('label[for="connect-ssh-host"]')?.textContent).toBe(
      'Host',
    );
  });

  it('connects to what was typed rather than to the empty row', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    fill('host', 'acter-ssh');
    fill('port', '2222');
    fill('user', 'acter');

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(false);
    byId('connect-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([
      { profile: 'Ssh', host: 'acter-ssh', port: 2222, user: 'acter' },
    ]);
  });

  /**
   * **The button follows the form** — reported by the user on 2026-08-26: "why is the
   * connect button ever enabled when information isn't complete?"
   *
   * A disabled button is itself the information: tabbing to it and hearing "unavailable"
   * says the form is not finished, without committing to anything. The old shape told you
   * only after a round trip you had to wait for. A8 decision 4 keeps the button live for a
   * kind this machine cannot *start* — which is different, because nothing you do in the
   * dialog changes that.
   */
  it('keeps Connect unavailable until there is something to connect to', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);

    fill('host', 'acter-ssh');
    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);

    fill('user', 'acter');
    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(false);
  });

  /**
   * **Enter obeys the same condition as the button** — reported by the user on 2026-08-26:
   * "if I press enter on ssh in the list (with all blank) I should hear nothing, but I
   * listen the error message." Enter reached `chosen` directly and never consulted the
   * button it was standing in for, so a disabled Connect was a lie told to whoever tabbed
   * to it.
   */
  it('does nothing when Enter is pressed on an incomplete form', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    byId('connect-dialog').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });

  /** And Enter works once the form names something to connect to. */
  it('connects on Enter once the form is complete', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    fill('host', 'acter-ssh');
    fill('user', 'acter');

    byId('connect-dialog').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
    await Promise.resolve();

    expect(attempted).toEqual([
      { profile: 'Ssh', host: 'acter-ssh', port: 22, user: 'acter' },
    ]);
  });

  /** And moving off the form gives the button back, for a kind that needs no form. */
  it('gives the button back on a kind that is startable as it stands', async () => {
    await dialog.open();
    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);

    byId('connect-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }),
    );

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(false);
  });
});

/**
 * **Nothing in the dialog can be pressed while a connection is being made.**
 *
 * Reported by the user on 2026-08-26: after submitting a password they were left focused
 * on the Connect button for the seconds the server took, and could press it again into the
 * attempt already in flight. For somebody navigating by focus, sitting on a control called
 * Connect *is* being told that connecting has not started.
 */
describe('while an attempt is running', () => {
  /** A connect action that does not finish until the test lets it. */
  function pending(): { attempts: number; finish: (worked: boolean) => void } {
    return { attempts: 0, finish: () => {} };
  }

  it('disables its controls, and refuses a second attempt into the first', async () => {
    const held = pending();
    let release: (worked: boolean) => void = () => {};
    dialog = new ConnectDialog(
      byId<HTMLDialogElement>('connect-dialog'),
      byId('connect-kinds'),
      byId('connect-panel-title'),
      byId('connect-panel-body'),
      connect,
      () => {
        held.attempts += 1;
        return new Promise<boolean>((resolve) => {
          release = resolve;
        });
      },
      announcer,
      { focus: () => {} },
      connecting,
      help,
    );
    await dialog.open();

    byId('connect-start').click();
    await Promise.resolve();

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);
    expect(byId<HTMLButtonElement>('connect-cancel').disabled).toBe(true);
    expect(byId('connect-dialog').getAttribute('aria-busy')).toBe('true');

    // A second press, of the kind a user makes when nothing seems to be happening.
    byId('connect-start').click();
    await Promise.resolve();
    expect(held.attempts).toBe(1);

    release(false);
    await Promise.resolve();
    await Promise.resolve();

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(false);
  });

  /**
   * And a refusal puts focus back where another can be chosen — reported by the same
   * pass, which was returned to the Cancel button. Decision 4 keeps this dialog open on
   * failure precisely so somebody can choose again.
   */
  it('returns focus to the kind list when an attempt is refused', async () => {
    succeeds = false;
    await dialog.open();

    byId('connect-start').click();
    await Promise.resolve();
    await Promise.resolve();

    expect(document.activeElement?.id).toBe('connect-kinds');
  });
});

/**
 * The checkbox that authorises a session being set up (spec B9.5, decision 9).
 *
 * **Its whole reason for being on this dialog rather than only inside the one that discloses
 * the command** is that unticking it has to be reachable without the dialog ever appearing.
 */
describe('setting the session up', () => {
  it('is ticked when the dialog opens, because that is the default', async () => {
    await dialog.open();

    const box = byId<HTMLInputElement>('connect-set-up');
    expect(box.checked).toBe(true);
  });

  it('is labelled with a whole sentence, because it is read aloud', () => {
    const label = document.querySelector('label[for="connect-set-up"]');

    expect(label?.textContent).toContain('set this session up');
  });

  it('carries a ticked box to whoever connects', async () => {
    await dialog.open();

    byId('connect-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['Yes']);
  });

  /** **Unticking it skips both the dialog and the setup**, which is what refusing durably is
   * until B8 has a profile to keep the answer in (decision 10). */
  it('carries an unticked box to whoever connects', async () => {
    await dialog.open();
    byId<HTMLInputElement>('connect-set-up').checked = false;

    byId('connect-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['No']);
  });

  /** It is read at the moment Connect is pressed rather than remembered, so a listener who
   * changes their mind after choosing a kind gets what the box says now. */
  it('is read when Connect is pressed rather than when the dialog opened', async () => {
    await dialog.open();
    byId<HTMLInputElement>('connect-set-up').checked = false;
    byId<HTMLInputElement>('connect-set-up').checked = true;

    byId('connect-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['Yes']);
  });
});

/**
 * **A kind's parameters start on nothing chosen** — reported by the user on 2026-08-30:
 * choosing WSL and pressing Enter connected to Ubuntu, because a `<select>` selects its
 * first option for you. Connecting to something nobody picked is worse than not connecting.
 */
describe('choosing what a kind needs', () => {
  it('starts on nothing chosen', async () => {
    await dialog.open();
    press('ArrowDown');

    const select = byId<HTMLSelectElement>('connect-variant');
    expect(select.value).toBe('');
    expect(select.options[select.selectedIndex]?.textContent).toBe('not chosen');
  });

  it('keeps Connect unavailable until one is chosen', async () => {
    await dialog.open();
    press('ArrowDown');

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);

    const select = byId<HTMLSelectElement>('connect-variant');
    select.value = '1';
    select.dispatchEvent(new Event('change'));

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(false);
  });

  /** And going back to nothing takes it away again: the rule is about what is chosen now. */
  it('takes Connect away again when the choice is taken back', async () => {
    await dialog.open();
    press('ArrowDown');
    const select = byId<HTMLSelectElement>('connect-variant');
    select.value = '1';
    select.dispatchEvent(new Event('change'));

    select.value = '';
    select.dispatchEvent(new Event('change'));

    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);
  });

  /** **Enter connects to nothing, and says why** — silence would leave a listener pressing
   * a key that does nothing, with no way to find out what is missing. */
  it('answers Enter with what is missing rather than with a connection', async () => {
    await dialog.open();
    press('ArrowDown');
    announcer.announcements = [];

    press('Enter');
    await Promise.resolve();

    expect(attempted).toEqual([]);
    expect(announcer.announcements).toEqual(['choose a distribution first']);
  });

  /** Choosing a kind again clears what the last one held: the panel is rebuilt, so the
   * choice cannot survive into a kind it was never made for. */
  it('clears the choice when the kind changes', async () => {
    await dialog.open();
    press('ArrowDown');
    const select = byId<HTMLSelectElement>('connect-variant');
    select.value = '1';
    select.dispatchEvent(new Event('change'));

    press('ArrowUp');
    press('ArrowDown');

    expect(byId<HTMLSelectElement>('connect-variant').value).toBe('');
    expect(byId<HTMLButtonElement>('connect-start').disabled).toBe(true);
  });
});

/**
 * **Where Enter goes while a connection is being made** — reported by the user on
 * 2026-08-30, who pressed Enter on a kind and was put back on the list of kinds. Being
 * returned to the control you have just acted on is what a dialog does when nothing
 * happened.
 */
describe('the connecting dialog', () => {
  it('names what is being connected to, kind and variant', async () => {
    await dialog.open();
    press('ArrowDown');
    const select = byId<HTMLSelectElement>('connect-variant');
    select.value = '1';
    select.dispatchEvent(new Event('change'));

    byId('connect-start').click();
    await Promise.resolve();

    expect(connecting.shown).toEqual(['WSL: Debian']);
  });

  /** A kind with nothing to choose between is named by itself. */
  it('names a kind that needs nothing by itself', async () => {
    await dialog.open();

    press('Enter');
    await Promise.resolve();

    expect(connecting.shown).toEqual(['Command Prompt']);
  });

  it('takes it away when the attempt succeeds', async () => {
    await dialog.open();

    press('Enter');
    await Promise.resolve();
    await Promise.resolve();

    expect(connecting.hidden).toBe(1);
  });

  /** And when it fails, before the listener is put back on the list to choose again. */
  it('takes it away when the attempt is refused', async () => {
    succeeds = false;
    await dialog.open();

    press('Enter');
    await Promise.resolve();
    await Promise.resolve();

    expect(connecting.hidden).toBe(1);
    expect(document.activeElement?.id).toBe('connect-kinds');
  });

  /** Nothing is shown for an Enter that cannot connect: there is no attempt to narrate. */
  it('is not shown for an Enter that connects to nothing', async () => {
    await dialog.open();
    press('ArrowDown');

    press('Enter');
    await Promise.resolve();

    expect(connecting.shown).toEqual([]);
  });
});

/**
 * **The Help button beside the set-up checkbox** (reported 2026-08-30). What the box turns
 * on is four sentences, and an announcement is not where any of them belong.
 */
describe('help with setting a session up', () => {
  it('opens the help topic at the section about the box', async () => {
    await dialog.open();

    byId('connect-set-up-help').click();

    expect(help.opened).toHaveLength(1);
    expect(help.opened[0]?.topic).toBe('help-setting-up');
  });

  /** And comes back to itself, because the dialog it opens sits on top of one that is
   * still there — the window underneath is inert. */
  it('comes back to the button it was opened from', async () => {
    await dialog.open();

    byId('connect-set-up-help').click();

    expect(help.opened[0]?.returnTo).toBe(byId('connect-set-up-help'));
  });

  it('connects to nothing', async () => {
    await dialog.open();

    byId('connect-set-up-help').click();
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });
});
