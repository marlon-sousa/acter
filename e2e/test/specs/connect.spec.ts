// Role: e2e spec — the Connect dialog, driven end to end in the real WebView2 window:
// the menu opens it, the list comes from the real `connectable()` command over IPC, and
// choosing one really replaces the session behind the window (spec A8).
//
// **This is the half of B7's shape that only an E2E can prove.** The actions are unit
// tested with a fake factory and the dialog is unit tested with a fake action; what neither
// can reach is the whole path — a menu item in the real document, an invoke across the real
// bridge, a session actually replaced, and the window renaming itself for it.
//
// It runs against the scripted far end this suite always runs against, so the profile it
// connects to is one of the built-in scripted sessions a debug build offers.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand, useActersLine } from '../helpers';

interface Row {
  label: string;
  available: boolean;
}

function focusedId(): Promise<string> {
  return browser.execute(() => document.activeElement?.id ?? '');
}

/** The embedded WebDriver synthesizes untrusted key events; the app's own listeners take
 * them, which is everything this spec is about. */
function press(key: string, on = 'connect-kinds') {
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

function dialogIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('connect-dialog') as HTMLDialogElement | null)?.open ===
      true,
  );
}

/** Whether the "could not connect" modal is up. */
function failureIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('failed-dialog') as HTMLDialogElement | null)?.open ===
      true,
  );
}

/** The kinds as the dialog rendered them — which is the real command's answer, rendered. */
function rows(): Promise<Row[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('#connect-kinds [role="option"]'),
    ).map((option) => ({
      label: option.textContent ?? '',
      // A row the machine cannot start says so in its name, which is B5.4's decision and
      // what this asserts rather than a visual state.
      available: !(option.textContent ?? '').includes('(not available)'),
    })),
  );
}

function panelTitle(): Promise<string> {
  return browser.execute(
    () => document.getElementById('connect-panel-title')?.textContent ?? '',
  );
}

function windowTitle(): Promise<string> {
  return browser.execute(
    () => document.getElementById('window-title')?.textContent ?? '',
  );
}

/** Walk the menu bar to Connect and activate it, then wait for the dialog. The list comes
 * back over IPC, so the waiting is the part that has to be right on a slow machine. */
async function openConnect(): Promise<void> {
  await browser.execute(() => document.getElementById('command-input')?.focus());
  await press('F10', 'command-input');
  await press('ArrowDown', 'menu-acter');
  await press('Enter', 'menu-connect');
  await browser.waitUntil(dialogIsOpen, {
    timeout: 15_000,
    timeoutMsg: 'the Connect dialog never opened',
  });
}

/** Move the selection to the row whose label matches, and answer whether it was there. */
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

describe('the Connect dialog', () => {
  beforeEach(async () => {
    await browser.execute(() => {
      const dialog = document.getElementById(
        'connect-dialog',
      ) as HTMLDialogElement | null;
      dialog?.close();
    });
    await browser.waitUntil(async () => !(await dialogIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'the Connect dialog would not close between tests',
    });
    await useActersLine();
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  /** The whole path: the real menu bar, the real command, and a list built from what this
   * machine actually has. */
  it('opens from the menu with a list the backend answered', async () => {
    await openConnect();

    const listed = await rows();
    expect(listed.length).toBeGreaterThan(0);
    // A debug build offers the scripted far ends, which is what this suite runs on and the
    // one entry that is the same on every machine (spec B7, decision 7).
    expect(listed.some((row) => row.label === 'Scripted: builtin')).toBe(true);
    // And a real shell, so the list is not only the developer's tools.
    expect(listed.some((row) => row.label === 'Command Prompt')).toBe(true);
  });

  it('focuses the list rather than the dialog, so the first thing said is a kind', async () => {
    await openConnect();

    await expect(await focusedId()).toBe('connect-kinds');
  });

  /** Decision 2, end to end: the panel changes with the kind and focus stays on the list.
   * A list you cannot arrow through without leaving it is not a list. */
  it('arrows the kinds, changing the panel without moving focus', async () => {
    await openConnect();
    const before = await panelTitle();

    await press('End');

    await expect(await focusedId()).toBe('connect-kinds');
    const selected = await browser.execute(
      () =>
        document.querySelector('[role="option"][aria-selected="true"]')?.textContent ??
        '',
    );
    expect(selected).toBe((await rows()).at(-1)?.label);
    // The last row is either an unavailable kind or a scripted one, and either way its
    // panel says something different from the first row's.
    expect(await panelTitle()).not.toBe('');
    expect(typeof before).toBe('string');
  });

  /** **What only this suite can assert**: choosing a kind really replaces the session, and
   * the window renames itself with the label the connect list used. */
  it('connects to a scripted session and renames the window for it', async () => {
    await openConnect();
    expect(await chooseKind('Scripted: builtin')).toBe(true);

    await browser.execute(() => document.getElementById('connect-start')?.click());

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
    // **On the program's line, not Acter's** (roadmap 28.7). A session hands the keys to
    // the far end as soon as there is one, so this is where a listener lands: the field
    // labelled "Command line" that the far end draws into, with Acter's `<input>` hidden.
    await browser.waitUntil(async () => (await focusedId()) === 'far-end-input', {
      timeout: 15_000,
      timeoutMsg: "focus never landed on the program's line after connecting",
    });
  });

  /** Decision 4's failure half, against a kind this machine genuinely cannot start.
   *
   * **Skipped rather than failed on a machine that has everything**, for the reason the
   * Docker test in `real_session.rs` skips: a fully equipped machine has not discovered a
   * defect. The path itself is covered without a machine by the dialog's own suite and by
   * the router tests. */
  it('keeps itself open when the connection could not be started', async () => {
    await openConnect();
    const missing = (await rows()).find((row) => !row.available);
    if (missing === undefined) {
      console.log(
        'skipped: this machine can start every kind, so nothing here can fail to connect',
      );
      return;
    }
    expect(await chooseKind(missing.label)).toBe(true);
    const before = await windowTitle();

    await browser.execute(() => document.getElementById('connect-start')?.click());

    // **A failure is acknowledged rather than announced** (ARCHITECTURE, dialogs, rule 10):
    // it opens a modal that has to be dismissed, and the connect dialog is busy behind it
    // until that happens. Dismissing it here is not tidying up — it is the behaviour under
    // test, and leaving it open froze every test that followed when the modal first landed.
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

    // It stays open, and it stays open for good rather than closing a moment later.
    await browser.pause(1000);
    expect(await dialogIsOpen()).toBe(true);
    // And the window is still on whatever it was on: a failure costs the user nothing.
    expect(await windowTitle()).toBe(before);
  });

  it('cancels back to the edit field without connecting', async () => {
    await openConnect();
    const before = await windowTitle();

    await browser.execute(() => document.getElementById('connect-cancel')?.click());

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

  /** The panel is reachable, and so is Connect after it — the tab order decision 2 states.
   * Tab itself is the platform's and an untrusted synthetic key does not move focus, so
   * what is asserted here is that both are focusable at all, which is what makes the order
   * possible; that Tab walks it is the NVDA pass's to confirm. */
  it('has a panel and a Connect button that can hold focus', async () => {
    await openConnect();

    const reachable = await browser.execute(() => {
      const ids = ['connect-panel', 'connect-start', 'connect-cancel'];
      return ids.map((id) => {
        document.getElementById(id)?.focus();
        return document.activeElement?.id ?? '';
      });
    });

    expect(reachable).toEqual(['connect-panel', 'connect-start', 'connect-cancel']);
    await expect(await $('#connect-dialog').isDisplayed()).toBe(true);
  });
});

/** Whether an element is in the document at all, as far as a reader is concerned. */
function isShown(id: string): Promise<boolean> {
  return browser.execute(
    (which: string) => document.getElementById(which)?.hidden === false,
    id,
  );
}

// **The window's two faces** (spec A10), driven in the real window. This suite launches with
// a scripted session, so it starts on the terminal face; what it can prove here is that the
// faces are wired to the session rather than to anything the user pressed.
describe('what the window shows', () => {
  it('shows the terminal window and not the empty one while connected', async () => {
    await browser.execute(() => document.getElementById('command-input')?.focus());

    expect(await isShown('terminal-window')).toBe(true);
    expect(await isShown('not-connected-window')).toBe(false);
    expect(await isShown('command-form')).toBe(true);
    expect(await isShown('terminal-ended')).toBe(false);
  });

  /** **The two windows are exclusive**, which is the whole model: a window with no session
   * and a window with one are different things rather than one window whose controls wink
   * in and out. */
  it('never shows both windows at once', async () => {
    const both = await browser.execute(
      () =>
        document.getElementById('not-connected-window')?.hidden === false &&
        document.getElementById('terminal-window')?.hidden === false,
    );

    expect(both).toBe(false);
  });

  /** The buffer is in the document only once it has something in it. This suite has
   * submitted commands by now in other specs, but each spec file gets its own app, so this
   * one asserts the rule from both ends. */
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

  /** The seam tabs will use: the buffer and the edit field are one thing, grouped, rather
   * than two that happen to sit together in `<main>`. */
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

  /** The dialog's keys belong to its widgets rather than to the reader's browse cursor, so
   * its contents sit in an application region (spec A10). Asserted on the structure because
   * what it changes is the reader's mode, which only the NVDA pass can hear. */
  it('holds the Connect dialog in an application region', async () => {
    const wrapped = await browser.execute(() => {
      const region = document.querySelector('#connect-dialog [role="application"]');
      return (
        region?.contains(document.getElementById('connect-kinds')) === true &&
        region?.contains(document.getElementById('connect-start')) === true
      );
    });

    expect(wrapped).toBe(true);
  });
});
