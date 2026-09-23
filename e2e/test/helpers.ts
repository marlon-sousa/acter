// Role: e2e helper — submit a command through the real form, as the app does.
//
// The embedded WebDriver (tauri-plugin-wdio-webdriver) sends key presses as untrusted
// KeyboardEvents, which never trigger implicit form submission, so these helpers call
// `requestSubmit` or dispatch the keydown the adapter listens for.

import { $, browser } from '@wdio/globals';

export async function submitCommand(text: string): Promise<void> {
  const input = await $('aria/Command input');
  await input.setValue(text);
  await browser.execute(() => {
    document.querySelector('form')?.requestSubmit();
  });
}

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

/** Connecting hands the keys back to the program, so a spec that starts a new session calls
 * this before typing into Acter's `<input>`. */
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
