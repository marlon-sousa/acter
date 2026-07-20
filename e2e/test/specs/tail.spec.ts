// Role: e2e spec — chunks arriving over time append to the same command block in
// order. The `tail` script (E2E config: two fast iterations) emits "tail line 1" then
// "tail line 2" under one block; this asserts they land in the same output region in
// arrival order.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('tail: live chunks append to one block in order', () => {
  it('appends both tail chunks under the same heading in order', async () => {
    await submitCommand('tail');

    const heading = await $('h2=tail');
    await heading.waitForExist({ timeout: 10_000 });

    await browser.waitUntil(
      async () => {
        const text = await browser.execute(() => {
          const headings = Array.from(document.querySelectorAll('#results h2'));
          const own = headings.find((el) => el.textContent === 'tail');
          return own?.nextElementSibling?.textContent ?? null;
        });
        // Concatenated in arrival order, same block.
        return text === 'tail line 1tail line 2';
      },
      {
        timeout: 10_000,
        timeoutMsg: 'both tail chunks did not append to the same block in order',
      },
    );
  });
});
