// @vitest-environment jsdom
// Role: test — the New connection dialog's behaviour.

import { beforeEach, describe, expect, it } from 'vitest';

import { panelSummary } from '../../src/adapters/connection_panel';
import { NewConnectionDialog } from '../../src/adapters/new_connection_dialog';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { ConnectApi } from '../../src/ports/connect_api';
import type { HelpView } from '../../src/ports/help_view';
import type {
  Connectable,
  Connected,
  LaunchRequest,
  ProfileId,
  SavedConnections,
  SetUp,
} from '../../src/protocol';

const SKELETON = `
<dialog id="new-connection-dialog" aria-labelledby="new-connection-title">
  <div role="application" aria-label="New connection">
    <h1 id="new-connection-title">New connection</h1>
    <ul id="new-kinds" role="listbox" aria-label="Connection kind" tabindex="0"></ul>
    <div id="new-panel" role="group" aria-labelledby="new-panel-title" tabindex="-1">
      <h2 id="new-panel-title">Options</h2>
      <div id="new-panel-body"></div>
    </div>
    <p>
      <input id="new-set-up" type="checkbox" checked />
      <label for="new-set-up"
        >Let Acter set this session up so it can tell you more about what you run</label
      >
    </p>
    <button id="new-set-up-help" type="button">Help with setting a session up</button>
    <button id="new-start" type="button">Connect</button>
    <button id="new-cancel" type="button">Cancel</button>
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

function terminal(): Connectable {
  return {
    id: { profile: 'Install', kind: 'Terminal', program: '/bin/zsh', provenance: 'zsh' },
    label: 'Terminal',
    available: true,
    instructions: null,
    variants: [
      {
        id: { profile: 'Install', kind: 'Terminal', program: '/bin/zsh', provenance: 'zsh' },
        label: 'zsh (default)',
        available: true,
        instructions: null,
      },
      {
        id: { profile: 'Install', kind: 'Terminal', program: '/bin/bash', provenance: 'bash' },
        label: 'bash',
        available: true,
        instructions: null,
      },
    ],
  };
}

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
  saved(): Promise<SavedConnections> {
    return Promise.resolve({ rows: [], unreadable: null });
  }
  saveConnection(): Promise<string> {
    throw new Error('New connection saves nothing: naming happens after it is up');
  }
  renameConnection(): Promise<string> {
    throw new Error('New connection renames nothing');
  }
  forgetConnection(): Promise<string> {
    throw new Error('New connection forgets nothing');
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
  drained(): Promise<void> {
    return Promise.resolve();
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

class FakeHelp implements HelpView {
  opened: { topic?: string; returnTo?: { focus(): void } }[] = [];
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void {
    this.opened.push(options ?? {});
  }
}

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

function chooseVariant(at: number): void {
  const list = byId('new-variant');
  for (let step = 0; step <= at; step += 1) {
    list.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
  }
}

function variantLabels(): string[] {
  return Array.from(
    byId('new-variant').querySelectorAll('[role="option"]'),
  ).map((option) => option.textContent ?? '');
}

function chosenVariant(): string | null {
  return (
    byId('new-variant').querySelector('[role="option"][aria-selected="true"]')
      ?.textContent ?? null
  );
}

let connect: FakeConnect;
let announcer: FakeAnnouncer;
let attempted: ProfileId[];
let asked: SetUp[];
let succeeds: boolean;
let returned: number;
let connecting: FakeConnecting;
let help: FakeHelp;
let dialog: NewConnectionDialog;
let origins: (string | null)[];

function make(): NewConnectionDialog {
  return new NewConnectionDialog(
    byId<HTMLDialogElement>('new-connection-dialog'),
    byId('new-kinds'),
    byId('new-panel-title'),
    byId('new-panel-body'),
    connect,
    (id, setUp, origin) => {
      attempted.push(id);
      asked.push(setUp);
      origins.push(origin);
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
    () => announcer.announce('connected'),
  );
}

beforeEach(() => {
  document.body.innerHTML = SKELETON;
  const element = byId<HTMLDialogElement>('new-connection-dialog');
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
  origins = [];
  succeeds = true;
  returned = 0;
  dialog = make();
});

function options(): string[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>('#new-kinds [role="option"]'),
  ).map((option) => option.textContent ?? '');
}

function selected(): string | undefined {
  return (
    document.querySelector<HTMLElement>('[role="option"][aria-selected="true"]')
      ?.textContent ?? undefined
  );
}

async function attemptEnds(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function press(key: string): void {
  byId('new-kinds').dispatchEvent(
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
    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(true);
  });

  it('asks the machine again on every open', async () => {
    await dialog.open();
    byId<HTMLDialogElement>('new-connection-dialog').close();
    await dialog.open();

    expect(connect.asked).toBe(2);
  });

  it('is not broken by being opened twice', async () => {
    await dialog.open();
    await dialog.open();

    expect(connect.asked).toBe(1);
  });

  it('says nothing about a panel that is empty when it opens', async () => {
    await dialog.open();

    expect(announcer.announcements).toEqual([]);
  });

  it('says nothing when it opens on a kind that can be started', async () => {
    await dialog.open();

    expect(announcer.announcements).toEqual([]);
  });

  it('names the selected option to the reader from the first render', async () => {
    await dialog.open();

    expect(byId('new-kinds').getAttribute('aria-activedescendant')).toBe(
      'new-kind-0',
    );
    expect(selected()).toBe('Command Prompt');
  });
});

describe('the panel (decision 2)', () => {
  it('holds nothing for a kind that needs nothing', async () => {
    await dialog.open();

    expect(byId('new-panel-title').textContent).toBe('no options');
    expect(byId('new-panel-body').children).toHaveLength(0);
  });

  it('holds the distributions for WSL, named without repeating the kind', async () => {
    await dialog.open();
    press('ArrowDown');

    expect(variantLabels()).toEqual(['Ubuntu', 'Debian']);
    expect(byId('new-panel-title').textContent).toBe('2 distributions');
  });

  it('holds what to do about a kind this machine cannot start', async () => {
    await dialog.open();
    press('End');

    expect(byId('new-panel-title').textContent).toBe('not available');
    expect(byId('new-panel-body').textContent).toContain('winget install');
  });

  it('says nothing about a panel holding an ordinary choice, and speaks for one that cannot be started', async () => {
    await dialog.open();
    announcer.announcements = [];

    press('ArrowDown');
    press('ArrowDown');

    expect(announcer.announcements).toEqual(['not available']);
  });

  it('counts the shells on a Mac as shells', () => {
    expect(panelSummary(terminal())).toBe('2 shells');
  });

  it('counts one shell without pluralising it', () => {
    const one = terminal();
    one.variants = one.variants.slice(0, 1);

    expect(panelSummary(one)).toBe('1 shell');
  });

  it('counts one distribution without pluralising it', () => {
    const one = wsl();
    one.variants = one.variants.slice(0, 1);

    expect(panelSummary(one)).toBe('1 distribution');
  });
});

describe('arrowing the kinds', () => {
  it('moves the selection and leaves focus on the list', async () => {
    await dialog.open();

    press('ArrowDown');

    expect(selected()).toBe('WSL');
    expect(byId('new-kinds').getAttribute('aria-activedescendant')).toBe(
      'new-kind-1',
    );
    expect(document.activeElement).toBe(byId('new-kinds'));
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

    byId('new-start').click();
    await attemptEnds();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'Cmd' }]);
    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(false);
    expect(returned).toBe(1);
  });

  it('starts the chosen distribution rather than the kind when the panel offered any', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant(1);

    byId('new-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Distribution', name: 'Debian' }]);
  });

  it('enter on the list connects, the way enter on a chosen row should', async () => {
    await dialog.open();

    press('Enter');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'Cmd' }]);
  });

  it('stays open when the connection could not be started', async () => {
    succeeds = false;
    await dialog.open();

    byId('new-start').click();
    await Promise.resolve();

    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(true);
    expect(returned).toBe(0);
  });

  it('still attempts a kind the machine cannot start, and lets the backend refuse it', async () => {
    succeeds = false;
    await dialog.open();
    press('End');

    byId('new-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(true);
  });
});

describe('leaving', () => {
  it('cancel closes it and puts focus back in the edit field', async () => {
    await dialog.open();

    byId('new-cancel').click();

    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(false);
    expect(returned).toBe(1);
    expect(attempted).toEqual([]);
  });

  it('closing by any route returns focus to the edit field', async () => {
    await dialog.open();

    byId<HTMLDialogElement>('new-connection-dialog').close();

    expect(returned).toBe(1);
  });
});

describe('keeping Tab inside', () => {
  it('cycles from the last control back to the first', async () => {
    await dialog.open();
    byId('new-cancel').focus();

    press2('Tab');

    expect(document.activeElement).toBe(byId('new-kinds'));
  });

  it('cycles backwards from the first control to the last', async () => {
    await dialog.open();
    byId('new-kinds').focus();

    press2('Tab', true);

    expect(document.activeElement).toBe(byId('new-cancel'));
  });

  it('walks forwards through every control in order', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant(1);
    byId('new-kinds').focus();

    const walked: string[] = [];
    for (let step = 0; step < 6; step += 1) {
      press2('Tab');
      walked.push(document.activeElement?.id ?? '');
    }

    expect(walked).toEqual([
      'new-variant',
      'new-set-up',
      'new-set-up-help',
      'new-start',
      'new-cancel',
      'new-kinds',
    ]);
  });
});

function press2(key: string, shift = false): void {
  byId('new-connection-dialog').dispatchEvent(
    new KeyboardEvent('keydown', { key, shiftKey: shift, bubbles: true }),
  );
}

describe('Enter as the default action', () => {
  function enterOn(id: string): void {
    byId(id).dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
  }

  it('connects from the distribution combo box', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant(1);

    enterOn('new-variant');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Distribution', name: 'Debian' }]);
  });

  it('connects from the panel', async () => {
    await dialog.open();
    press('End');

    enterOn('new-panel');
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
  });

  it('leaves a button to answer its own Enter', async () => {
    await dialog.open();

    enterOn('new-cancel');
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });
});

describe('a kind whose variants can be missing', () => {
  beforeEach(() => {
    connect.rows = [powershell(), cmd()];
  });

  it('names the panel after what its variants are', async () => {
    await dialog.open();

    expect(byId('new-panel-title').textContent).toBe('2 editions');
    expect(byId('new-variant').getAttribute('aria-label')).toBe('Edition');
  });

  it('lists the missing edition, saying so in its name', async () => {
    await dialog.open();

    expect(variantLabels()).toEqual([
      'Windows PowerShell',
      'PowerShell 7 (not available)',
    ]);
  });

  it('shows no instructions while an available edition is chosen', async () => {
    await dialog.open();

    expect(byId('new-panel-body').querySelector('[data-instructions]')).toBeNull();
  });

  it('shows and announces what to do when the missing edition is chosen', async () => {
    await dialog.open();
    announcer.announcements = [];

    chooseVariant(1);

    const said = byId('new-panel-body').querySelector('[data-instructions]');
    expect(said?.textContent).toContain('winget install');
    expect((said as HTMLElement).tabIndex).toBe(0);
    expect(announcer.announcements).toEqual(['not available']);
  });

  it('still attempts the missing edition, and lets the backend refuse it', async () => {
    succeeds = false;
    await dialog.open();
    chooseVariant(1);

    byId('new-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([{ profile: 'Shell', kind: 'PowerShellSeven' }]);
    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(true);
  });
});

describe('the kind that is a form', () => {
  function ssh(): Connectable {
    return {
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

  function fill(name: string, value: string): void {
    const field = byId<HTMLInputElement>(`new-ssh-${name}`);
    field.value = value;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  }

  it('says nothing about the panel, and does not count the boxes', async () => {
    await dialog.open();
    announcer.announcements.length = 0;

    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(announcer.announcements).toEqual([]);
    expect(byId('new-panel-title').textContent).toBe('Connection details');
  });

  it('offers a host, a port and an account, with the usual port filled in', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(byId<HTMLInputElement>('new-ssh-host').value).toBe('');
    expect(byId<HTMLInputElement>('new-ssh-port').value).toBe('22');
    expect(byId<HTMLInputElement>('new-ssh-user').value).toBe('');
    expect(document.querySelector('label[for="new-ssh-host"]')?.textContent).toBe(
      'Host',
    );
  });

  it('connects to what was typed rather than to the empty row', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    fill('host', 'acter-ssh');
    fill('port', '2222');
    fill('user', 'acter');

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(false);
    byId('new-start').click();
    await Promise.resolve();

    expect(attempted).toEqual([
      { profile: 'Ssh', host: 'acter-ssh', port: 2222, user: 'acter' },
    ]);
  });

  it('keeps Connect unavailable until there is something to connect to', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);

    fill('host', 'acter-ssh');
    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);

    fill('user', 'acter');
    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(false);
  });

  it('does nothing when Enter is pressed on an incomplete form', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );

    byId('new-connection-dialog').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });

  it('connects on Enter once the form is complete', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    fill('host', 'acter-ssh');
    fill('user', 'acter');

    byId('new-connection-dialog').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    );
    await Promise.resolve();

    expect(attempted).toEqual([
      { profile: 'Ssh', host: 'acter-ssh', port: 22, user: 'acter' },
    ]);
  });

  it('gives the button back on a kind that is startable as it stands', async () => {
    await dialog.open();
    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);

    byId('new-kinds').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }),
    );

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(false);
  });
});

describe('while an attempt is running', () => {
  function pending(): { attempts: number; finish: (worked: boolean) => void } {
    return { attempts: 0, finish: () => {} };
  }

  it('disables its controls, and refuses a second attempt into the first', async () => {
    const held = pending();
    let release: (worked: boolean) => void = () => {};
    dialog = new NewConnectionDialog(
      byId<HTMLDialogElement>('new-connection-dialog'),
      byId('new-kinds'),
      byId('new-panel-title'),
      byId('new-panel-body'),
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

    byId('new-start').click();
    await Promise.resolve();

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);
    expect(byId<HTMLButtonElement>('new-cancel').disabled).toBe(true);
    expect(byId('new-connection-dialog').getAttribute('aria-busy')).toBe('true');

    byId('new-start').click();
    await Promise.resolve();
    expect(held.attempts).toBe(1);

    release(false);
    await Promise.resolve();
    await Promise.resolve();

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(false);
  });

  it('returns focus to the kind list when an attempt is refused', async () => {
    succeeds = false;
    await dialog.open();

    byId('new-start').click();
    await Promise.resolve();
    await Promise.resolve();

    expect(document.activeElement?.id).toBe('new-kinds');
  });
});

describe('setting the session up', () => {
  it('is ticked when the dialog opens, because that is the default', async () => {
    await dialog.open();

    const box = byId<HTMLInputElement>('new-set-up');
    expect(box.checked).toBe(true);
  });

  it('is labelled with a whole sentence, because it is read aloud', () => {
    const label = document.querySelector('label[for="new-set-up"]');

    expect(label?.textContent).toContain('set this session up');
  });

  it('carries a ticked box to whoever connects', async () => {
    await dialog.open();

    byId('new-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['Yes']);
  });

  it('carries an unticked box to whoever connects', async () => {
    await dialog.open();
    byId<HTMLInputElement>('new-set-up').checked = false;

    byId('new-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['No']);
  });

  it('is read when Connect is pressed rather than when the dialog opened', async () => {
    await dialog.open();
    byId<HTMLInputElement>('new-set-up').checked = false;
    byId<HTMLInputElement>('new-set-up').checked = true;

    byId('new-start').dispatchEvent(new Event('click'));
    await Promise.resolve();

    expect(asked).toEqual(['Yes']);
  });
});

describe('choosing what a kind needs', () => {
  it('starts with none of them chosen', async () => {
    await dialog.open();
    press('ArrowDown');

    expect(chosenVariant()).toBeNull();
    expect(byId('new-variant').getAttribute('aria-activedescendant')).toBeNull();
  });

  it('takes the first row on the first press, not before it', async () => {
    await dialog.open();
    press('ArrowDown');

    chooseVariant(0);

    expect(chosenVariant()).toBe('Ubuntu');
    expect(byId('new-variant').getAttribute('aria-activedescendant')).toBe(
      'new-variant-0',
    );
  });

  it('keeps Connect unavailable until one is chosen', async () => {
    await dialog.open();
    press('ArrowDown');

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);

    chooseVariant(1);

    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(false);
  });

  it('answers Enter with what is missing rather than with a connection', async () => {
    await dialog.open();
    press('ArrowDown');
    announcer.announcements = [];

    press('Enter');
    await Promise.resolve();

    expect(attempted).toEqual([]);
    expect(announcer.announcements).toEqual(['choose a distribution first']);
  });

  it('clears the choice when the kind changes', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant(1);

    press('ArrowUp');
    press('ArrowDown');

    expect(chosenVariant()).toBeNull();
    expect(byId<HTMLButtonElement>('new-start').disabled).toBe(true);
  });
});

describe('the connecting dialog', () => {
  it('names what is being connected to, kind and variant', async () => {
    await dialog.open();
    press('ArrowDown');
    chooseVariant(1);

    byId('new-start').click();
    await Promise.resolve();

    expect(connecting.shown).toEqual(['WSL: Debian']);
  });

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

  it('takes it away when the attempt is refused', async () => {
    succeeds = false;
    await dialog.open();

    press('Enter');
    await Promise.resolve();
    await Promise.resolve();

    expect(connecting.hidden).toBe(1);
    expect(document.activeElement?.id).toBe('new-kinds');
  });

  it('is not shown for an Enter that connects to nothing', async () => {
    await dialog.open();
    press('ArrowDown');

    press('Enter');
    await Promise.resolve();

    expect(connecting.shown).toEqual([]);
  });
});

describe('naming the far end afterwards', () => {
  it('closes, re-establishes the baseline, and only then says where the listener is', async () => {
    await dialog.open();

    press('Enter');
    await attemptEnds();

    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(false);
    expect(connecting.hidden).toBe(1);
    expect(announcer.said).toEqual(['document returned', 'connected']);
  });

  it('says nothing of the kind when the attempt is refused', async () => {
    succeeds = false;
    await dialog.open();

    press('Enter');
    await attemptEnds();

    expect(byId<HTMLDialogElement>('new-connection-dialog').open).toBe(true);
    expect(announcer.said).toEqual([]);
  });
});

describe('help with setting a session up', () => {
  it('opens the help topic at the section about the box', async () => {
    await dialog.open();

    byId('new-set-up-help').click();

    expect(help.opened).toHaveLength(1);
    expect(help.opened[0]?.topic).toBe('help-setting-up');
  });

  it('comes back to the button it was opened from', async () => {
    await dialog.open();

    byId('new-set-up-help').click();

    expect(help.opened[0]?.returnTo).toBe(byId('new-set-up-help'));
  });

  it('connects to nothing', async () => {
    await dialog.open();

    byId('new-set-up-help').click();
    await Promise.resolve();

    expect(attempted).toEqual([]);
  });
});
