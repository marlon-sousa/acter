// Role: e2e spec — chunks arriving over time append to the same command block, and
// none of them is lost. The `tail` rule delivers ten identical lines one delivery at a
// time; every one must land under the one heading the submission opened.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

// The lines accumulated under the `tail` heading, or null while no such block exists.
function tailBlockLines(): Promise<string[] | null> {
  return browser.execute(() => {
    const headings = Array.from(document.querySelectorAll('#results h2'));
    const own = headings.find((el) => el.textContent === 'tail');
    const output = own?.nextElementSibling;
    if (output === null || output === undefined) {
      return null;
    }
    return Array.from(output.children).map((row) => row.textContent ?? '');
  });
}

describe('tail: live chunks append to one block', () => {
  it('appends every tail chunk under the same heading, losing none', async () => {
    await submitCommand('tail');

    const heading = await $('h2=tail');
    await heading.waitForExist({ timeout: 10_000 });

    await browser.waitUntil(
      async () => {
        const lines = await tailBlockLines();
        return (
          lines !== null &&
          lines.length === 10 &&
          lines.every((line) => line === 'tail line')
        );
      },
      {
        timeout: 10_000,
        timeoutMsg: 'the ten tail chunks did not all reach one block',
      },
    );

    const headings = await browser.execute(
      () =>
        Array.from(document.querySelectorAll('#results h2')).filter(
          (el) => el.textContent === 'tail',
        ).length,
    );
    expect(headings).toBe(1);
  });
});
