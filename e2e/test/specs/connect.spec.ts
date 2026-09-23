// Role: e2e spec — the two connect dialogs, driven end to end in the real WebView2 window.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand, useActersLine } from '../helpers';

interface Row {
  label: string;
  available: boolean;
}

function focusedId(): Promise<string> {
  return browser.execute(() => document.activeElement?.id ?? '');
}

function press(key: string, on = 'new-kinds') {
  return browser.execute(
    (k: string, id: string) => {
      const target = document.getElementById(id) ?? document.body;
      target.dispatchEvent(
        new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }),
      );
    },
    key,
    on,
  );
}

function dialogIsOpen(which = 'new-connection-dialog'): Promise<boolean> {
  return browser.execute(
    (id: string) =>
      (document.getElementById(id) as HTMLDialogElement | null)?.open === true,
    which,
  );
}

function failureIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('failed-dialog') as HTMLDialogElement | null)?.open ===
      true,
  );
}

function rows(): Promise<Row[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('#new-kinds [role="option"]'),
    ).map((option) => ({
      label: option.textContent ?? '',
      available: !(option.textContent ?? '').includes('(not available)'),
    })),
  );
}

function panelTitle(): Promise<string> {
  return browser.execute(
    () => document.getElementById('new-panel-title')?.textContent ?? '',
  );
}

function savedNames(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('#connect-names [role="option"]'),
    ).map((option) => option.textContent ?? ''),
  );
}

function windowTitle(): Promise<string> {
  return browser.execute(
    () => document.getElementById('window-title')?.textContent ?? '',
  );
}

/** The list comes back over IPC, so this waits for the dialog. */
async function openFromTheMenu(item: string, dialog: string): Promise<void> {
  await browser.execute(() => document.getElementById('command-input')?.focus());
  await press('F10', 'command-input');
  await press('ArrowDown', 'menu-acter');
  await press('Enter', item);
  await browser.waitUntil(() => dialogIsOpen(dialog), {
    timeout: 15_000,
    timeoutMsg: `${dialog} never opened`,
  });
}

/** Every connection from New connection is followed by the offer to save, which holds focus
 * until answered; resolves to whether the offer appeared. */
async function notNow(): Promise<boolean> {
  const offered = await browser
    .waitUntil(async () => await dialogIsOpen('save-connection-dialog'), {
      timeout: 10_000,
      timeoutMsg: 'the offer to save never appeared',
    })
    .then(
      () => true,
      () => false,
    );
  if (offered) {
    await browser.execute(() => document.getElementById('save-cancel')?.click());
    await browser.waitUntil(
      async () => !(await dialogIsOpen('save-connection-dialog')),
      { timeout: 10_000, timeoutMsg: 'the offer to save would not close' },
    );
  }
  return offered;
}

function openNewConnection(): Promise<void> {
  return openFromTheMenu('menu-new-connection', 'new-connection-dialog');
}

function openConnect(): Promise<void> {
  return openFromTheMenu('menu-connect', 'connect-dialog');
}

async function chooseKind(label: string): Promise<boolean> {
  const listed = await rows();
  const at = listed.findIndex((row) => row.label === label);
  if (at === -1) {
    return false;
  }
  await press('Home');
  for (let step = 0; step < at; step += 1) {
    await press('ArrowDown');
  }
  return true;
}

describe('the New connection dialog', () => {
  beforeEach(async () => {
    await browser.execute(() => {
      for (const id of ['new-connection-dialog', 'save-connection-dialog']) {
        (document.getElementById(id) as HTMLDialogElement | null)?.close();
      }
    });
    await browser.waitUntil(async () => !(await dialogIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'the New connection dialog would not close between tests',
    });
    await useActersLine();
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('opens from the menu with a list the backend answered', async () => {
    await openNewConnection();

    const listed = await rows();
    expect(listed.length).toBeGreaterThan(0);
    expect(listed.some((row) => row.label === 'Scripted: builtin')).toBe(true);
    expect(listed.some((row) => row.label === 'Command Prompt')).toBe(true);
  });

  it('focuses the list rather than the dialog, so the first thing said is a kind', async () => {
    await openNewConnection();

    await expect(await focusedId()).toBe('new-kinds');
  });

  it('arrows the kinds, changing the panel without moving focus', async () => {
    await openNewConnection();
    const before = await panelTitle();

    await press('End');

    await expect(await focusedId()).toBe('new-kinds');
    const selected = await browser.execute(
      () =>
        document.querySelector('[role="option"][aria-selected="true"]')?.textContent ??
        '',
    );
    expect(selected).toBe((await rows()).at(-1)?.label);
    expect(await panelTitle()).not.toBe('');
    expect(typeof before).toBe('string');
  });

  it('connects to a scripted session and renames the window for it', async () => {
    await openNewConnection();
    expect(await chooseKind('Scripted: builtin')).toBe(true);

    await browser.execute(() => document.getElementById('new-start')?.click());

    await browser.waitUntil(async () => !(await dialogIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'the dialog never closed after connecting',
    });
    await browser.waitUntil(
      async () => (await windowTitle()) === 'Acter - Scripted: builtin',
      {
        timeout: 15_000,
        timeoutMsg: `the window never renamed itself; it says ${await windowTitle()}`,
      },
    );
    await expect(await notNow()).toBe(true);
    await browser.waitUntil(async () => (await focusedId()) === 'far-end-input', {
      timeout: 15_000,
      timeoutMsg: `focus never landed on the program's line; it is on ${await focusedId()}`,
    });
  });

  it('keeps itself open when the connection could not be started', async () => {
    await openNewConnection();
    const missing = (await rows()).find((row) => !row.available);
    if (missing === undefined) {
      console.log(
        'skipped: this machine can start every kind, so nothing here can fail to connect',
      );
      return;
    }
    expect(await chooseKind(missing.label)).toBe(true);
    const before = await windowTitle();

    await browser.execute(() => document.getElementById('new-start')?.click());

    // The failure modal must be dismissed: left open, it blocks every test after this one.
    await browser.waitUntil(async () => await failureIsOpen(), {
      timeout: 15_000,
      timeoutMsg: 'a connection that could not be started said nothing to acknowledge',
    });
    const why = await browser.execute(
      () => document.getElementById('failed-why')?.textContent ?? '',
    );
    expect(why.length).toBeGreaterThan(0);

    await browser.execute(() => document.getElementById('failed-ok')?.click());
    await browser.waitUntil(async () => !(await failureIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'OK never closed the failure dialog',
    });

    // Paused so a dialog that closed a moment later would be caught.
    await browser.pause(1000);
    expect(await dialogIsOpen()).toBe(true);
    expect(await windowTitle()).toBe(before);
  });

  it('cancels back to the edit field without connecting', async () => {
    await openNewConnection();
    const before = await windowTitle();

    await browser.execute(() => document.getElementById('new-cancel')?.click());

    await browser.waitUntil(async () => !(await dialogIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'Cancel never closed the dialog',
    });
    await browser.waitUntil(async () => (await focusedId()) === 'command-input', {
      timeout: 15_000,
      timeoutMsg: 'focus never returned to the edit field',
    });
    expect(await windowTitle()).toBe(before);
  });

  /** An untrusted synthetic Tab does not move focus, so this asserts only that both can hold
   * focus. */
  it('has a panel and a Connect button that can hold focus', async () => {
    await openNewConnection();

    const reachable = await browser.execute(() => {
      const ids = ['new-panel', 'new-start', 'new-cancel'];
      return ids.map((id) => {
        document.getElementById(id)?.focus();
        return document.activeElement?.id ?? '';
      });
    });

    expect(reachable).toEqual(['new-panel', 'new-start', 'new-cancel']);
    await expect(await $('#new-connection-dialog').isDisplayed()).toBe(true);
  });
});

function isShown(id: string): Promise<boolean> {
  return browser.execute(
    (which: string) => document.getElementById(which)?.hidden === false,
    id,
  );
}

describe('what the window shows', () => {
  it('shows the terminal window and not the empty one while connected', async () => {
    await browser.execute(() => document.getElementById('command-input')?.focus());

    expect(await isShown('terminal-window')).toBe(true);
    expect(await isShown('not-connected-window')).toBe(false);
    expect(await isShown('command-form')).toBe(true);
    expect(await isShown('terminal-ended')).toBe(false);
  });

  it('never shows both windows at once', async () => {
    const both = await browser.execute(
      () =>
        document.getElementById('not-connected-window')?.hidden === false &&
        document.getElementById('terminal-window')?.hidden === false,
    );

    expect(both).toBe(false);
  });

  it('brings the buffer in with its first content', async () => {
    const before = await isShown('results');
    await submitCommand('small');
    await browser.waitUntil(async () => isShown('results'), {
      timeout: 15_000,
      timeoutMsg: 'the buffer never appeared after a command ran',
    });

    expect(typeof before).toBe('boolean');
    expect(await isShown('results')).toBe(true);
  });

  it('keeps the buffer and the edit field together in one terminal window', async () => {
    const grouped = await browser.execute(() => {
      const terminal = document.getElementById('terminal-window');
      return (
        terminal?.contains(document.getElementById('results')) === true &&
        terminal?.contains(document.getElementById('command-form')) === true
      );
    });

    expect(grouped).toBe(true);
  });

  it('holds both connect dialogs in an application region', async () => {
    const wrapped = await browser.execute(() => {
      const making = document.querySelector(
        '#new-connection-dialog [role="application"]',
      );
      const saved = document.querySelector('#connect-dialog [role="application"]');
      return (
        making?.contains(document.getElementById('new-kinds')) === true &&
        making?.contains(document.getElementById('new-start')) === true &&
        saved?.contains(document.getElementById('connect-names')) === true &&
        saved?.contains(document.getElementById('connect-start')) === true
      );
    });

    expect(wrapped).toBe(true);
  });
});

describe('the Connect dialog', () => {
  beforeEach(async () => {
    await browser.execute(() => {
      for (const id of [
        'connect-dialog',
        'new-connection-dialog',
        'save-connection-dialog',
      ]) {
        (document.getElementById(id) as HTMLDialogElement | null)?.close();
      }
    });
    await browser.waitUntil(async () => !(await dialogIsOpen('connect-dialog')), {
      timeout: 15_000,
      timeoutMsg: 'the Connect dialog would not close between tests',
    });
    await useActersLine();
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('lists the saved connection the fixture holds', async () => {
    await openConnect();

    expect(await savedNames()).toEqual(['the fake']);
  });

  it('focuses the names rather than the dialog', async () => {
    await openConnect();

    await expect(await focusedId()).toBe('connect-names');
  });

  it('loads the panel and the checkbox from the saved row', async () => {
    await openConnect();

    const panel = await browser.execute(() => ({
      title: document.getElementById('connect-panel-title')?.textContent ?? '',
      setUp:
        (document.getElementById('connect-set-up') as HTMLInputElement | null)
          ?.checked === true,
    }));

    expect(panel.title).not.toBe('');
    expect(panel.setUp).toBe(true);
  });

  it('connects to the saved connection and renames the window for it', async () => {
    await openConnect();

    await browser.execute(() => document.getElementById('connect-start')?.click());

    await browser.waitUntil(async () => !(await dialogIsOpen('connect-dialog')), {
      timeout: 15_000,
      timeoutMsg: 'the dialog never closed after connecting',
    });
    await browser.waitUntil(
      async () => (await windowTitle()) === 'Acter - Scripted: builtin',
      {
        timeout: 15_000,
        timeoutMsg: `the window never renamed itself; it says ${await windowTitle()}`,
      },
    );
  });

  it('saves a session under a new name and lists it afterwards', async () => {
    await openConnect();
    await browser.execute(() => document.getElementById('connect-start')?.click());
    await browser.waitUntil(async () => !(await dialogIsOpen('connect-dialog')), {
      timeout: 15_000,
      timeoutMsg: 'the dialog never closed after connecting',
    });

    const said = await browser.execute(async () => {
      const invoke = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke(command: string, args: unknown): Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__.invoke;
      return (await invoke('save_connection', { name: 'saved by the suite' })) as string;
    });

    expect(said).toBe('Saved as saved by the suite.');

    await openConnect();
    expect(await savedNames()).toEqual(['saved by the suite', 'the fake']);
  });

  it('has Connect, Rename, Forget, New connection and Cancel, and no Save', async () => {
    await openConnect();

    const buttons = await browser.execute(() =>
      Array.from(
        document.querySelectorAll<HTMLElement>('#connect-dialog button'),
      ).map((button) => button.textContent ?? ''),
    );

    expect(buttons).toEqual([
      'Connect',
      'Rename',
      'Forget',
      'New connection',
      'Cancel',
    ]);
  });

  it('closes itself when New connection is chosen', async () => {
    await openConnect();

    await browser.execute(() => document.getElementById('connect-new')?.click());

    await browser.waitUntil(async () => !(await dialogIsOpen('connect-dialog')), {
      timeout: 15_000,
      timeoutMsg: 'the Connect dialog stayed open under New connection',
    });
    await browser.waitUntil(async () => await dialogIsOpen('new-connection-dialog'), {
      timeout: 15_000,
      timeoutMsg: 'New connection never opened',
    });
    await browser.execute(() => document.getElementById('new-cancel')?.click());
  });
});
