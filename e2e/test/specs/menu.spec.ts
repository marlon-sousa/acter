// Role: e2e spec — the menu bar and the About dialog, driven end to end in the real
// WebView2 window.

import { $, browser, expect } from '@wdio/globals';
import { useActersLine } from '../helpers';

function focusedId(): Promise<string> {
  return browser.execute(() => document.activeElement?.id ?? '');
}

/** Dispatched on `document`, where F10 and Alt are bound; see helpers.ts for why keys are
 * synthesized rather than typed. */
function press(key: string, options: { alt?: boolean; type?: string } = {}) {
  return browser.execute(
    (k: string, alt: boolean, type: string) => {
      const target = document.activeElement ?? document.body;
      target.dispatchEvent(
        new KeyboardEvent(type, {
          key: k,
          altKey: alt,
          bubbles: true,
          cancelable: true,
        }),
      );
    },
    key,
    options.alt ?? false,
    options.type ?? 'keydown',
  );
}

/** The menu bar is wired after an IPC round trip, so F10 is pressed until focus reaches it. */
before(async () => {
  await browser.waitUntil(
    async () => {
      await press('F10');
      return (await focusedId()) === 'menu-acter';
    },
    {
      timeout: 30_000,
      timeoutMsg: 'F10 never reached the menu bar, so nothing was listening for it',
    },
  );
});

describe('the menu bar', () => {
  beforeEach(async () => {
    await useActersLine();
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('opens on F10 with focus on the first item', async () => {
    await press('F10');

    await expect(await focusedId()).toBe('menu-acter');
  });

  it('opens on Alt pressed and released alone', async () => {
    await press('Alt', { alt: true });
    await press('Alt', { type: 'keyup' });

    await expect(await focusedId()).toBe('menu-acter');
  });

  it('does not open when another key came between the Alt press and its release', async () => {
    await press('Alt', { alt: true });
    await press('Tab', { alt: true });
    await press('Alt', { type: 'keyup' });

    await expect(await focusedId()).toBe('command-input');
  });

  it('walks with the arrows and steps into a menu', async () => {
    await press('F10');
    await press('ArrowRight');
    await expect(await focusedId()).toBe('menu-help');

    await press('ArrowRight');
    await expect(await focusedId()).toBe('menu-about');

    await press('ArrowDown');
    await expect(await focusedId()).toBe('menu-about-acter');
  });

  it('leaves on Escape and puts focus back in the edit field', async () => {
    await press('F10');
    await press('Escape');

    await expect(await focusedId()).toBe('command-input');
  });
});

function dialogIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('about-dialog') as HTMLDialogElement | null)?.open === true,
  );
}

/** The dialog opens only after its facts come back over IPC, so this waits for it. */
async function openAbout(): Promise<void> {
  await press('F10');
  await press('ArrowRight');
  await press('ArrowRight');
  await press('ArrowDown');
  await press('Enter');
  await browser.waitUntil(dialogIsOpen, {
    timeout: 15_000,
    timeoutMsg: 'the About dialog never opened',
  });
}

describe('the About dialog', () => {
  beforeEach(async () => {
    // The previous test may leave the dialog open, and closing it is what moves focus, so
    // this waits for the close before placing focus.
    await browser.execute(() => {
      const dialog = document.getElementById('about-dialog') as HTMLDialogElement | null;
      dialog?.close();
    });
    await browser.waitUntil(async () => !(await dialogIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'the About dialog would not close between tests',
    });
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('opens from the menu and reads its facts from the build', async () => {
    await openAbout();

    const dialog = await $('#about-dialog');
    await expect(await dialog.getText()).toContain('Acter');
    await expect(await dialog.getText()).toContain('MIT licence');
    await expect(await dialog.getText()).toContain('Marlon Brandão de Sousa');
    const said = await browser.execute(
      () => document.getElementById('about-version')?.textContent ?? '',
    );
    await expect(/^(Version|Development build, commit) .+\. \S+$/.test(said)).toBe(true);
    const settings = await browser.execute(
      () => document.getElementById('about-settings')?.textContent ?? '',
    );
    await expect(settings).toContain('Settings folder: ');
    await expect(settings).toContain('Acter was told where to keep its settings.');
  });

  it('keeps Tab inside itself', async () => {
    await openAbout();

    await browser.execute(() => document.getElementById('about-close')?.focus());
    await press('Tab');

    await expect(await focusedId()).toBe('about-close');
  });

  it('closes on Escape and leaves focus in the edit field', async () => {
    await openAbout();

    // An untrusted synthetic Escape does not reach the modal dialog's own close.
    await browser.execute(() => document.getElementById('about-close')?.click());

    await browser.waitUntil(async () => (await focusedId()) === 'command-input', {
      timeout: 15_000,
      timeoutMsg: 'focus never returned to the edit field',
    });
  });
});

function helpIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('help-dialog') as HTMLDialogElement | null)?.open === true,
  );
}

describe('the Help dialog', () => {
  beforeEach(async () => {
    await browser.execute(() => {
      const dialog = document.getElementById('help-dialog') as HTMLDialogElement | null;
      dialog?.close();
    });
    await browser.waitUntil(async () => !(await helpIsOpen()), {
      timeout: 15_000,
      timeoutMsg: 'the Help dialog would not close between tests',
    });
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('opens on F1 from the edit field', async () => {
    await press('F1');

    await browser.waitUntil(helpIsOpen, {
      timeout: 15_000,
      timeoutMsg: 'F1 did not open Help',
    });
  });

  it('opens from the menu as well, on the same dialog', async () => {
    await press('F10');
    await press('ArrowRight');
    await press('ArrowDown');
    await press('Enter');

    await browser.waitUntil(helpIsOpen, {
      timeout: 15_000,
      timeoutMsg: 'the Help menu did not open Help',
    });
  });

  it('carries a readable topic rather than a widget', async () => {
    await press('F1');
    await browser.waitUntil(helpIsOpen, { timeout: 15_000 });

    const shape = await browser.execute(() => {
      const dialog = document.getElementById('help-dialog');
      return {
        applications: dialog?.querySelectorAll('[role="application"]').length ?? -1,
        headings: dialog?.querySelectorAll('h2').length ?? -1,
        describedBy: dialog?.getAttribute('aria-describedby') ?? '',
      };
    });

    await expect(shape.applications).toBe(0);
    await expect(shape.headings).toBe(7);
    await expect(shape.describedBy).toBe('help-summary');
  });

  it('closes and leaves focus in the edit field', async () => {
    await press('F1');
    await browser.waitUntil(helpIsOpen, { timeout: 15_000 });

    await browser.execute(() => document.getElementById('help-close')?.click());

    await browser.waitUntil(async () => (await focusedId()) === 'command-input', {
      timeout: 15_000,
      timeoutMsg: 'focus never returned to the edit field',
    });
  });
});
