// Role: e2e helper — submit a command through the real form, as the app does.
//
// The embedded WebDriver (tauri-plugin-wdio-webdriver) synthesizes key presses as
// JavaScript KeyboardEvents. Untrusted events never trigger the browser's native
// implicit form submission, so "type + Enter" fills the field and stops there.
// Instead we call form.requestSubmit(), which fires the same cancelable `submit`
// event a real Enter produces — the entire app path from the submit event onward
// (keyboard adapter -> controller -> invoke -> buffer + announcer DOM) is exercised.
// Native Enter-to-submit is browser machinery, not app code; real keystrokes are
// covered by the manual NVDA pass.

import { $, browser } from '@wdio/globals';

export async function submitCommand(text: string): Promise<void> {
  // Located by accessible name, not CSS: fails if the computed accessible name
  // ("Command input") ever regresses.
  const input = await $('aria/Command input');
  await input.setValue(text);
  await browser.execute(() => {
    document.querySelector('form')?.requestSubmit();
  });
}

// Ctrl+C as the app must receive it: on the edit field, which is the only element that
// listens for it (DESIGN layer 2 — the session hears a keystroke only while that field
// has focus). The embedded WebDriver's synthesized key events are untrusted, exactly as
// recorded above for Enter, so this dispatches the keydown the adapter listens for;
// everything from that listener onward is the app's own code path.
//
// Aiming it at the field rather than at `document` is the point of the helper: a
// document-level dispatch would pass even if the app had bound the key globally, which is
// precisely the thing DESIGN forbids.
export async function pressCtrlC(): Promise<void> {
  await browser.execute(() => {
    document.getElementById('command-input')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'c',
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
  });
}

/**
 * Give Acter's own line the keys, if the program has them (roadmap 28.7).
 *
 * `wdio.conf.ts` does this once for the whole suite and explains why. It is needed again
 * here because **connecting hands them back**: the default is per session, so any spec that
 * opens the Connect dialog and starts a new one lands on the program's line again, where
 * Acter's `<input>` is hidden and cannot take focus.
 */
export async function useActersLine(): Promise<void> {
  const showing = async (): Promise<boolean | undefined> =>
    await browser.execute(
      () => document.getElementById('far-end-line')?.hidden === false,
    );
  if ((await showing()) !== true) {
    return;
  }
  await browser.execute(() => {
    document.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'K',
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
  });
  await browser.waitUntil(async () => (await showing()) !== true, {
    timeout: 15_000,
    timeoutMsg: 'Ctrl+Shift+K did not bring the keys back to Acter',
  });
}

/** The debug recorder's tape: what crossed the port, in arrival order (spec A3.2). */
export function debugTape(): Promise<Array<{ kind: string; what: string }>> {
  return browser.execute(
    () =>
      (
        window as unknown as {
          __acterDebug?: { entries(): Array<{ kind: string; what: string }> };
        }
      ).__acterDebug?.entries() ?? [],
  );
}
