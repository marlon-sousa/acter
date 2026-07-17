// Role: e2e spec — the app launches and a submitted command produces its block.
// The edit field is located by its ACCESSIBLE NAME ("Command input"), not a CSS
// selector, so this test fails if the accessible name ever breaks.
//
// The suite shares one app session (see wdio.conf.ts), so the buffer may already
// hold blocks from other specs; assertions address this spec's own block by its
// text, never by position.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('smoke: launch and submit a command', () => {
  it('finds the input by accessible name and renders the command block', async () => {
    const input = await $('aria/Command input');
    await expect(input).toBeExisting();

    await submitCommand('hello world');

    // An h2 with the submitted command appears in the results region.
    const heading = await $('h2=hello world');
    await heading.waitForExist({ timeout: 10_000 });

    // The response (echo returns the text unchanged) renders directly under it.
    const responseText = await browser.execute(() => {
      const headings = Array.from(document.querySelectorAll('#results h2'));
      const own = headings.find((el) => el.textContent === 'hello world');
      return own?.nextElementSibling?.textContent ?? null;
    });
    expect(responseText).toBe('hello world');
  });
});
