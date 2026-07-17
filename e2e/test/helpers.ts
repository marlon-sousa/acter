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
