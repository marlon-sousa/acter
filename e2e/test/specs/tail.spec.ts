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

// The lines accumulated under the `tail` heading, or null while no such block exists.
//
// **Read as elements rather than as one string since 28** (decision 8). The buffer applies
// revisions by line id now, so each line of output is an element of its own and the
// separators the service used to put *inside* the text are gone — which is what a listener
// arrowing the buffer meets, one line at a time. Concatenating `textContent` across those
// siblings would run them together, so what this asserts is what the DOM actually holds.
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

    // Ten deliveries of "tail line", each its own line in the buffer.
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
