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
