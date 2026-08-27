// Role: e2e spec — the menu bar and the About dialog, driven end to end in the real
// WebView2 window.
//
// **This suite could not have existed a day earlier.** A7's spec said, of the native menu
// bar it was written for, that no suite in this project could open it: `MockRuntime` does
// not execute native webview libraries and WebDriver drives the webview only. The menu bar
// moved into the document because a native one freezes NVDA, and this is the half of that
// change that pays for itself in tests.
//
// Elements are located by role and accessible name wherever a name exists, so this fails
// if the semantics regress rather than only if the markup moves.

import { $, browser, expect } from '@wdio/globals';

/** Where focus actually is, as an id — the question every assertion here asks. */
function focusedId(): Promise<string> {
  return browser.execute(() => document.activeElement?.id ?? '');
}

/** The embedded WebDriver synthesizes untrusted key events, exactly as `helpers.ts`
 * records for Enter and Ctrl+C. F10 and Alt are bound on `document`, so dispatching there
 * exercises the app's own listener and everything after it. */
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

describe('the menu bar', () => {
  beforeEach(async () => {
    // Every test starts from where the user lives.
    await browser.execute(() => document.getElementById('command-input')?.focus());
  });

  it('opens on F10 with focus on the first item', async () => {
    await press('F10');

    await expect(await focusedId()).toBe('menu-acter');
  });

  /** Alt is answered on keyup and disarmed by anything in between, which is what keeps
   * Alt+Tab and Alt+F4 working. Both halves are exercised here in the real webview,
   * because it is the real webview that receives Alt first. */
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

/** Is the dialog open right now? Asked of the element rather than of the DOM's shape,
 * because `open` is what `showModal` sets and what `close` clears. */
function dialogIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('about-dialog') as HTMLDialogElement | null)?.open === true,
  );
}

/** Walk the menu to About Acter and activate it, then wait for the dialog to be open.
 * Factored out because three tests need the same six steps, and because the waiting is
 * the part that has to be right: CI is slower than this machine, and the facts come back
 * over IPC before the dialog is shown.
 *
 * **Two ArrowRights since A13**, Help having arrived between Acter and About. A helper
 * that counts keypresses to reach a menu is a helper that breaks when a menu is added,
 * which is what happened — so the walk asserts nothing and the caller asserts the dialog,
 * and the test above is the one that pins the bar's order. */
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
    // Every test here starts from a closed dialog and a focused edit field, and *waits*
    // for that rather than assuming it: the previous test leaves the dialog open, closing
    // is what returns focus, and a test that began before either had happened would fail
    // for a reason that has nothing to do with what it is testing.
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

  /** The whole path: menu bar, into a menu, activate, and a dialog carrying facts that
   * came from the Rust side rather than from the page. */
  it('opens from the menu and reads four facts from the build', async () => {
    await openAbout();

    const dialog = await $('#about-dialog');
    // The name is filled by the adapter from the `about` command; the HTML ships empty.
    await expect(await dialog.getText()).toContain('Acter');
    await expect(await dialog.getText()).toContain('Version');
    await expect(await dialog.getText()).toContain('MIT licence');
    await expect(await dialog.getText()).toContain('Marlon Brandão de Sousa');
  });

  /** Measured through NVDA before it was fixed: Tab left the only control for the
   * dialog's own document, and the reader dropped back into browse mode. */
  it('keeps Tab inside itself', async () => {
    await openAbout();

    await browser.execute(() => document.getElementById('about-close')?.focus());
    await press('Tab');

    await expect(await focusedId()).toBe('about-close');
  });

  it('closes on Escape and leaves focus in the edit field', async () => {
    await openAbout();

    // Escape on a modal dialog is the platform's own, and an untrusted synthetic key does
    // not reach it — so this closes the dialog the way its own close button does, which is
    // the path the app owns. That Escape closes it is the NVDA pass's to confirm.
    await browser.execute(() => document.getElementById('about-close')?.click());

    await browser.waitUntil(async () => (await focusedId()) === 'command-input', {
      timeout: 15_000,
      timeoutMsg: 'focus never returned to the edit field',
    });
  });
});

/** Is the Help dialog open right now? Asked of the element for `dialogIsOpen`'s reason. */
function helpIsOpen(): Promise<boolean> {
  return browser.execute(
    () =>
      (document.getElementById('help-dialog') as HTMLDialogElement | null)?.open === true,
  );
}

/** **This suite exists because the e2e gap is what let A13 through red** (spec A13). The
 * unit suites caught the menu bar gaining a third item and the e2e one did not, because
 * nothing here exercised Help at all — so the failure arrived as a CI surprise on a PR
 * that had been driven with a screen reader and called done. Help has two ways in and both
 * are now walked end to end in the real webview. */
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

  /** F1 is bound on the document rather than on any control, because the sentence that
   * sends a user here is announced while the window may be showing anything. */
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

  /** The topic is prose to be read, so it must not be an application region and must keep
   * its headings — the two things a reader needs to arrow it and skim it. Asserted on the
   * shipped markup rather than on a skeleton, which is what the unit suite can only
   * approximate. */
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
    await expect(shape.headings).toBe(3);
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
