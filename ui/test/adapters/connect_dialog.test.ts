// @vitest-environment jsdom
// Role: test — the Connect dialog's behaviour since spec 26: what it lists, what the panel
// is loaded with, what a listener hears on arrowing, and what Rename and Forget do to the
// row they are on (decisions 12 to 16).
//
// **The dialog this file used to drive is `new_connection_dialog.test.ts` now.** File →
// Connect opens the list of saved names, and File → New connection opens the list of kinds.

import { beforeEach, describe, expect, it } from 'vitest';

import {
  ConnectDialog,
  NOTHING_SAVED,
  forgetting,
} from '../../src/adapters/connect_dialog';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { ConnectApi } from '../../src/ports/connect_api';
import type {
  Connectable,
  Connected,
  LaunchRequest,
  ProfileId,
  SavedConnections,
  SavedRow,
  SetUp,
} from '../../src/protocol';

// The dialog's static skeleton, copied from views/main_window.html. It is a copy on
// purpose: what this file asserts is the behaviour over that structure, and the structure
// itself is what the E2E spec and the NVDA pass drive in the real document.
const SKELETON = `
<dialog id="connect-dialog" aria-labelledby="connect-title">
  <div role="application" aria-label="Connect">
    <h1 id="connect-title">Connect</h1>
    <ul id="connect-names" role="listbox" aria-label="Saved connections" tabindex="0"></ul>
    <p id="connect-empty" tabindex="0" hidden></p>
    <div id="connect-panel" role="group" aria-labelledby="connect-panel-title" tabindex="-1">
      <h2 id="connect-panel-title">Options</h2>
      <div id="connect-panel-body"></div>
    </div>
    <p>
      <input id="connect-set-up" type="checkbox" checked />
      <label for="connect-set-up">Let Acter set this session up</label>
    </p>
    <button id="connect-start" type="button">Connect</button>
    <button id="connect-rename" type="button">Rename</button>
    <button id="connect-forget" type="button">Forget</button>
    <button id="connect-new" type="button">New connection</button>
    <button id="connect-cancel" type="button">Cancel</button>
  </div>
</dialog>
<input id="command-input" />
`;

function row(name: string, over: Partial<SavedRow> = {}): SavedRow {
  return {
    name,
    id: { profile: 'Shell', kind: 'Cmd' },
    summary: 'Command Prompt',
    set_up: 'Yes',
    line_owner: 'FarEnd',
    available: true,
    instructions: null,
    ...over,
  };
}

function ssh(name: string, port = 22): SavedRow {
  return row(name, {
    id: { profile: 'Ssh', host: 'example.org', port, user: 'marlon' },
    summary: `SSH, marlon at example.org${port === 22 ? '' : `, port ${port}`}`,
  });
}

function kinds(): Connectable[] {
  return [
    {
      id: { profile: 'Shell', kind: 'Cmd' },
      label: 'Command Prompt',
      available: true,
      instructions: null,
      variants: [],
    },
    {
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
    },
    {
      id: { profile: 'Ssh', host: '', port: 22, user: '' },
      label: 'SSH',
      available: true,
      instructions: null,
      variants: [],
    },
  ];
}

class FakeConnect implements ConnectApi {
  rows: SavedRow[] = [];
  unreadable: string | null = null;
  /** How many times the list was asked for, so "fresh every time" is assertable. */
  asked = 0;
  renamed: [string, string][] = [];
  forgot: string[] = [];
  /** The sentence a rename or a forget rejects with, when a test wants a refusal. */
  refuses: string | null = null;

  connectable(): Promise<Connectable[]> {
    return Promise.resolve(kinds());
  }
  use(): Promise<Connected> {
    throw new Error('the dialog connects through its action, not through this port');
  }
  connected(): Promise<Connected | null> {
    return Promise.resolve(null);
  }
  saved(): Promise<SavedConnections> {
    this.asked += 1;
    return Promise.resolve({ rows: this.rows, unreadable: this.unreadable });
  }
  saveConnection(): Promise<string> {
    throw new Error('there is no Save in this dialog');
  }
  renameConnection(from: string, to: string): Promise<string> {
    if (this.refuses !== null) {
      return Promise.reject(this.refuses);
    }
    this.renamed.push([from, to]);
    this.rows = this.rows.map((saved) =>
      saved.name === from ? { ...saved, name: to.trim() } : saved,
    );
    return Promise.resolve(`${from} is now called ${to.trim()}.`);
  }
  forgetConnection(name: string): Promise<string> {
    if (this.refuses !== null) {
      return Promise.reject(this.refuses);
    }
    this.forgot.push(name);
    this.rows = this.rows.filter((saved) => saved.name !== name);
    return Promise.resolve(`${name} is no longer saved.`);
  }
  offerToSave(): Promise<boolean> {
    return Promise.resolve(true);
  }
  stopOfferingToSave(): Promise<void> {
    return Promise.resolve();
  }
  requestedAtLaunch(): Promise<LaunchRequest | null> {
    return Promise.resolve(null);
  }
}

class FakeAnnouncer implements AnnouncerView {
  announcements: string[] = [];
  said: string[] = [];
  announce(text: string): void {
    this.announcements.push(text);
    this.said.push(text);
  }
  documentReturned(): void {
    this.said.push('document returned');
  }
}

class FakeConnecting {
  shown: string[] = [];
  hidden = 0;
  show(label: string): void {
    this.shown.push(label);
  }
  hide(): void {
    this.hidden += 1;
  }
}

/** The two dialogs this one opens on top of itself, each answering what a test says. */
class FakeAsking {
  renameWith: string | null = null;
  forgetIt = false;
  askedToRename: string[] = [];
  askedToForget: string[] = [];
  rename(name: string): Promise<string | null> {
    this.askedToRename.push(name);
    return Promise.resolve(this.renameWith);
  }
  forget(asking: string): Promise<boolean> {
    this.askedToForget.push(asking);
    return Promise.resolve(this.forgetIt);
  }
}

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

/** The names, in the order the list holds them. */
function names(): string[] {
  return Array.from(
    byId('connect-names').querySelectorAll('[role="option"]'),
  ).map((option) => option.textContent ?? '');
}

/** Which one is selected, or null while none is. */
function chosen(): string | null {
  return (
    byId('connect-names').querySelector('[role="option"][aria-selected="true"]')
      ?.textContent ?? null
  );
}

/** Arrow down through the list, the way a person does. */
function arrowDown(times = 1): void {
  for (let step = 0; step < times; step += 1) {
    byId('connect-names').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
  }
}

let connect: FakeConnect;
let announcer: FakeAnnouncer;
let connecting: FakeConnecting;
let asking: FakeAsking;
let attempted: { id: ProfileId; setUp: SetUp; origin: string | null }[];
let succeeds: boolean;
let returned: number;
let newConnections: number;
let dialog: ConnectDialog;

function make(): ConnectDialog {
  return new ConnectDialog(
    byId<HTMLDialogElement>('connect-dialog'),
    byId('connect-names'),
    byId('connect-empty'),
    byId('connect-panel-title'),
    byId('connect-panel-body'),
    connect,
    (id, setUp, origin) => {
      attempted.push({ id, setUp, origin });
      return Promise.resolve(succeeds);
    },
    announcer,
    {
      focus: () => {
        returned += 1;
      },
    },
    connecting,
    asking,
    () => {
      newConnections += 1;
    },
    () => announcer.announce('connected'),
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
  asking = new FakeAsking();
  attempted = [];
  succeeds = true;
  returned = 0;
  newConnections = 0;
  dialog = make();
});

describe('opening', () => {
  /**
   * **The list is the saved names** (decision 12), asked for afresh so a connection saved
   * in another window is there without a restart.
   */
  it('lists the saved names the backend answered', async () => {
    connect.rows = [row('Ada'), ssh('work laptop', 2222)];

    await dialog.open();

    expect(names()).toEqual(['Ada', 'work laptop']);
    expect(connect.asked).toBe(1);
  });

  /**
   * **Focus lands on the first name**, so the everyday case is open, arrow, Enter — and
   * the first thing a listener hears is a connection's name rather than the dialog's own
   * title.
   */
  it('focuses the list, with the first name chosen', async () => {
    connect.rows = [row('Ada'), row('Bob')];

    await dialog.open();

    expect(document.activeElement?.id).toBe('connect-names');
    expect(chosen()).toBe('Ada');
  });

  /**
   * **And says nothing on the way in.** The listbox announces the name focus lands on;
   * an announcement on top of that is the second utterance A8 measured being heard twice.
   */
  it('announces nothing of its own as it opens', async () => {
    connect.rows = [row('Ada')];

    await dialog.open();

    expect(announcer.announcements).toEqual([]);
  });

  /**
   * **With nothing saved it still opens** (decision 16), with the list replaced by a
   * sentence that says what to do about itself and focus on New connection.
   */
  it('says there is nothing saved yet and puts focus on New connection', async () => {
    await dialog.open();

    expect(byId('connect-empty').hidden).toBe(false);
    expect(byId('connect-empty').textContent).toBe(NOTHING_SAVED);
    expect(byId('connect-names').hidden).toBe(true);
    expect(document.activeElement?.id).toBe('connect-new');
  });

  /** The empty sentence says what to do, and it is one a reader can speak. */
  it('has an empty sentence that is a whole one and names the way out', () => {
    expect(NOTHING_SAVED.endsWith('.')).toBe(true);
    expect(NOTHING_SAVED).toContain('New connection');
    expect(NOTHING_SAVED).not.toContain('  ');
  });

  /**
   * **A document that would not parse is reported where the empty list would be**
   * (decisions 9 and 16). There is nothing saved either way, and the difference is whether
   * the person should go looking for a file — so the backend's sentence takes its place.
   */
  it('says what went wrong with the document instead of saying the list is empty', async () => {
    connect.unreadable =
      'Acter could not understand the connections it had saved, so it has started with none.';

    await dialog.open();

    expect(byId('connect-empty').textContent).toBe(connect.unreadable);
    expect(byId('connect-empty').textContent).not.toBe(NOTHING_SAVED);
    expect(document.activeElement?.id).toBe('connect-new');
  });
});

describe('arrowing onto a name', () => {
  /** **One line, and it is the domain's** (decision 13): the kind and what identifies it. */
  it('announces the kind and what identifies it', async () => {
    connect.rows = [row('Ada'), ssh('work laptop', 2222)];
    await dialog.open();

    arrowDown();

    expect(announcer.announcements).toEqual(['SSH, marlon at example.org, port 2222']);
  });

  /** And never moves focus, which is what makes a list a list. */
  it('moves the selection without moving focus', async () => {
    connect.rows = [row('Ada'), row('Bob')];
    await dialog.open();

    arrowDown();

    expect(chosen()).toBe('Bob');
    expect(document.activeElement?.id).toBe('connect-names');
  });

  /**
   * **The panel is loaded with the row's values** (decision 13): the SSH form filled in
   * rather than empty, which is the whole difference from the New connection dialog.
   */
  it('fills the SSH form in from the saved connection', async () => {
    connect.rows = [ssh('work laptop', 2222)];

    await dialog.open();

    expect(byId<HTMLInputElement>('saved-ssh-host').value).toBe('example.org');
    expect(byId<HTMLInputElement>('saved-ssh-port').value).toBe('2222');
    expect(byId<HTMLInputElement>('saved-ssh-user').value).toBe('marlon');
  });

  /** And the distribution already selected, for the kind that has a list. */
  it('selects the saved distribution in the panel', async () => {
    connect.rows = [
      row('linux', {
        id: { profile: 'Distribution', name: 'Debian' },
        summary: 'WSL, Debian',
      }),
    ];

    await dialog.open();

    const selected = byId('saved-variant').querySelector(
      '[role="option"][aria-selected="true"]',
    );
    expect(selected?.textContent).toBe('Debian');
  });

  /** And the checkbox as it was saved, which is the other of the two settings. */
  it('sets the checkbox from the saved connection', async () => {
    connect.rows = [row('quiet', { set_up: 'No' }), row('loud', { set_up: 'Yes' })];

    await dialog.open();
    expect(byId<HTMLInputElement>('connect-set-up').checked).toBe(false);

    arrowDown();
    expect(byId<HTMLInputElement>('connect-set-up').checked).toBe(true);
  });

  /**
   * **A row that cannot be started says so and shows what to do** (decision 13), rather
   * than being dropped from a list that would then teach a listener Acter forgot it.
   */
  it('says a row is not available and puts its instructions in the panel', async () => {
    connect.rows = [
      row('Ada'),
      row('gone', {
        id: { profile: 'Distribution', name: 'Fedora' },
        summary: 'WSL, Fedora',
        available: false,
        instructions: 'The WSL distribution Fedora is not installed on this computer.',
      }),
    ];
    await dialog.open();

    arrowDown();

    expect(announcer.announcements).toEqual(['not available']);
    expect(byId('connect-panel-body').textContent).toContain('Fedora is not installed');
  });
});

describe('connecting', () => {
  /**
   * **The panel's current values, with the row's name as the origin** (decisions 11 and
   * 14): editing the port connects to that port and writes nothing.
   */
  it('connects with what the panel now holds, as the saved connection', async () => {
    connect.rows = [ssh('work laptop', 2222)];
    await dialog.open();
    const port = byId<HTMLInputElement>('saved-ssh-port');
    port.value = '2200';
    port.dispatchEvent(new Event('input', { bubbles: true }));

    byId('connect-start').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();

    expect(attempted).toHaveLength(1);
    expect(attempted[0]?.id).toEqual({
      profile: 'Ssh',
      host: 'example.org',
      port: 2200,
      user: 'marlon',
    });
    expect(attempted[0]?.origin).toBe('work laptop');
  });

  /** Enter connects from anywhere in the dialog that is not a button, as today. */
  it('connects on Enter from the list', async () => {
    connect.rows = [row('Ada')];
    await dialog.open();

    byId('connect-names').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
    await Promise.resolve();
    await Promise.resolve();

    expect(attempted).toHaveLength(1);
  });

  /** The connecting dialog is named with the connection's own name, not a kind's label. */
  it('names the saved connection in the dialog that says it is connecting', async () => {
    connect.rows = [row('work laptop')];
    await dialog.open();

    byId('connect-start').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();

    expect(connecting.shown).toEqual(['work laptop']);
  });

  /**
   * **The far end is named only once the dialog is out of the way** (roadmap 13.3), and
   * the region is given a wordless change to lose first.
   */
  it('closes, tells the announcer the document is back, and only then says what it is on', async () => {
    connect.rows = [row('Ada')];
    await dialog.open();

    byId('connect-start').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(announcer.said).toEqual(['document returned', 'connected']);
    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(false);
  });

  /** A failure leaves the dialog open and focus back on the list, not on a button. */
  it('stays open on failure with focus back on the names', async () => {
    connect.rows = [row('Ada')];
    succeeds = false;
    await dialog.open();

    byId('connect-start').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(true);
    expect(document.activeElement?.id).toBe('connect-names');
  });
});

describe('renaming', () => {
  /** **Focus returns to the renamed row** (decision 15), and the sentence is said once. */
  it('renames the row and puts focus back on it', async () => {
    connect.rows = [row('Ada'), row('Bob')];
    asking.renameWith = 'Zoe';
    await dialog.open();
    arrowDown();

    byId('connect-rename').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(asking.askedToRename).toEqual(['Bob']);
    expect(connect.renamed).toEqual([['Bob', 'Zoe']]);
    expect(chosen()).toBe('Zoe');
    expect(document.activeElement?.id).toBe('connect-names');
    expect(announcer.announcements).toContain('Bob is now called Zoe.');
  });

  /** Cancelling changes nothing and leaves the listener where they were. */
  it('changes nothing when the rename dialog is cancelled', async () => {
    connect.rows = [row('Ada')];
    asking.renameWith = null;
    await dialog.open();

    byId('connect-rename').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(connect.renamed).toEqual([]);
    expect(names()).toEqual(['Ada']);
    expect(document.activeElement?.id).toBe('connect-names');
  });

  /**
   * **A refusal is the backend's sentence, announced** — the name rule and the collision
   * are decided in one place, and this says what that place answered.
   */
  it('announces the backend refusal and leaves the list alone', async () => {
    connect.rows = [row('Ada'), row('Bob')];
    asking.renameWith = 'Ada';
    connect.refuses =
      'A connection named Ada already exists. Choose another name, or forget that one first.';
    await dialog.open();
    arrowDown();

    byId('connect-rename').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(announcer.announcements).toContain(connect.refuses);
    expect(names()).toEqual(['Ada', 'Bob']);
    expect(document.activeElement?.id).toBe('connect-names');
  });
});

describe('forgetting', () => {
  /** **It asks once**, and the question names the row and says what does not change. */
  it('asks before it forgets, naming the row', async () => {
    connect.rows = [row('Ada')];
    asking.forgetIt = false;
    await dialog.open();

    byId('connect-forget').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(asking.askedToForget).toEqual([forgetting('Ada')]);
    expect(connect.forgot).toEqual([]);
    expect(names()).toEqual(['Ada']);
  });

  /** The question is a whole one, and it says what is *not* touched. */
  it('asks a question a listener can act on', () => {
    const asked = forgetting('work laptop');

    expect(asked).toContain('work laptop');
    expect(asked).toContain('Nothing on the computer it connected to changes.');
    expect(asked).not.toContain('  ');
  });

  /**
   * **Focus lands on the row that follows** (decision 15), because a listener who removed
   * a row is still working through the list.
   */
  it('removes the row and lands on the one that follows', async () => {
    connect.rows = [row('Ada'), row('Bob'), row('Cleo')];
    asking.forgetIt = true;
    await dialog.open();
    arrowDown();

    byId('connect-forget').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(connect.forgot).toEqual(['Bob']);
    expect(names()).toEqual(['Ada', 'Cleo']);
    expect(chosen()).toBe('Cleo');
    expect(document.activeElement?.id).toBe('connect-names');
    expect(announcer.announcements).toContain('Bob is no longer saved.');
  });

  /** Forgetting the last row lands on the one before it rather than nowhere. */
  it('lands on the row before when there is no row after', async () => {
    connect.rows = [row('Ada'), row('Bob')];
    asking.forgetIt = true;
    await dialog.open();
    arrowDown();

    byId('connect-forget').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(chosen()).toBe('Ada');
  });

  /** **And on New connection when the list is empty**, which is decision 16's landing. */
  it('lands on New connection when the last one is forgotten', async () => {
    connect.rows = [row('Ada')];
    asking.forgetIt = true;
    await dialog.open();

    byId('connect-forget').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();

    expect(names()).toEqual([]);
    expect(byId('connect-empty').hidden).toBe(false);
    expect(document.activeElement?.id).toBe('connect-new');
    expect(announcer.announcements).toContain('Ada is no longer saved.');
  });
});

describe('the other two buttons', () => {
  /**
   * **New connection closes this dialog first** (decision 15): dialogs do not stack, so
   * Cancel from there returns to the window rather than to this list.
   */
  it('closes itself before opening New connection', async () => {
    connect.rows = [row('Ada')];
    await dialog.open();

    byId('connect-new').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(false);
    expect(newConnections).toBe(1);
  });

  it('cancels back to the window without connecting', async () => {
    connect.rows = [row('Ada')];
    await dialog.open();

    byId('connect-cancel').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(byId<HTMLDialogElement>('connect-dialog').open).toBe(false);
    expect(attempted).toEqual([]);
    expect(returned).toBe(1);
  });

  /** **There is no Save here** (decision 15): saving happens from a live session. */
  it('has no Save button, because there is one way to save', () => {
    expect(byId<HTMLDialogElement>('connect-dialog').querySelector('#connect-save')).toBe(
      null,
    );
  });
});

/** Let the promises a click starts run out, which is three microtask turns deep here. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 8; turn += 1) {
    await Promise.resolve();
  }
}
