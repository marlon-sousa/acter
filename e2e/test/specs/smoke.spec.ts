// Role: e2e spec — the app launches, attaches to the fake session automatically, and a
// submitted scenario name produces its command block with the scripted output.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('smoke: launch and submit a scenario', () => {
  it('finds the input by accessible name and renders the small scenario block', async () => {
    const input = await $('aria/Command input');
    await expect(input).toBeExisting();

    await submitCommand('small');

    const heading = await $('h2=small');
    await heading.waitForExist({ timeout: 10_000 });

    await browser.waitUntil(
      async () => {
        const text = await browser.execute(() => {
          const headings = Array.from(document.querySelectorAll('#results h2'));
          const own = headings.find((el) => el.textContent === 'small');
          return own?.nextElementSibling?.textContent ?? null;
        });
        return text === 'hello from acter';
      },
      { timeout: 10_000, timeoutMsg: 'scripted output never appeared under the block' },
    );
  });
});
