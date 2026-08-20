// Role: e2e spec — chunks arriving over time append to the same command block, and
// none of them is lost. The `tail` rule delivers ten identical lines one delivery at a
// time; every one must land under the one heading the submission opened.
//
// Identical lines are the transcript's, not a simplification here, and they are why this
// asserts a count rather than a sequence: with ten copies of the same text, "in order" is
// not observable through the DOM. What IS observable — and what the block model actually
// promises — is that a command opens exactly one block, that late chunks join it instead
// of starting another, and that ten deliveries produce ten lines.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

// The text accumulated under the `tail` heading, or null while no such block exists.
function tailBlockText(): Promise<string | null> {
  return browser.execute(() => {
    const headings = Array.from(document.querySelectorAll('#results h2'));
    const own = headings.find((el) => el.textContent === 'tail');
    return own?.nextElementSibling?.textContent ?? null;
  });
}

describe('tail: live chunks append to one block', () => {
  it('appends every tail chunk under the same heading, losing none', async () => {
    await submitCommand('tail');

    const heading = await $('h2=tail');
    await heading.waitForExist({ timeout: 10_000 });

    // Ten deliveries of "tail line", separated as the service separates lines.
    const expected = Array.from({ length: 10 }, () => 'tail line').join('\n');
    await browser.waitUntil(async () => (await tailBlockText()) === expected, {
      timeout: 10_000,
      timeoutMsg: 'the ten tail chunks did not all reach one block',
    });

    // One block, not one per chunk: a second heading would mean the later deliveries
    // were attributed to a command of their own.
    const headings = await browser.execute(
      () =>
        Array.from(document.querySelectorAll('#results h2')).filter(
          (el) => el.textContent === 'tail',
        ).length,
    );
    expect(headings).toBe(1);
  });
});
