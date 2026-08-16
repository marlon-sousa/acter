// Role: e2e spec — the reproducibility hook. The `forever` script never ends on its
// own; typing `stop` must halt it, announce the pinned stop phrasing, and leave the
// session usable. This is the scenario that makes every other endless script safe to
// run in a suite: without it, `forever` outlives the spec that started it.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

// The text accumulated under a command's h2, or null while no such block exists.
function blockTextOf(command: string): Promise<string | null> {
  return browser.execute((name: string) => {
    const headings = Array.from(document.querySelectorAll('#results h2'));
    const own = headings.find((el) => el.textContent === name);
    return own?.nextElementSibling?.textContent ?? null;
  }, command);
}

describe('stop: halting an endless scenario', () => {
  it('stops forever, announces it, and leaves the session usable', async () => {
    await submitCommand('forever');

    // The E2E config paces `forever` at 20ms, so the block grows continuously. Wait
    // until it is genuinely running before stopping it.
    await browser.waitUntil(
      async () => ((await blockTextOf('forever')) ?? '').includes('still working'),
      { timeout: 10_000, timeoutMsg: 'forever never reached its quiet accumulation loop' },
    );

    await submitCommand('stop');

    // Pinned string (A3.1 decision 5). The live region empties on an idle timer, so
    // this must be observed promptly — waitUntil polls fast enough.
    await browser.waitUntil(
      async () => {
        const announced = await browser.execute(
          () => document.getElementById('announcer')?.textContent ?? '',
        );
        return announced.includes('command stopped');
      },
      { timeout: 10_000, timeoutMsg: 'the stop was never announced' },
    );

    // The decisive assertion: output stopped. Sample, wait well past several 20ms
    // intervals, and sample again — a still-running script would have grown.
    const settled = await blockTextOf('forever');
    await browser.pause(500);
    expect(await blockTextOf('forever')).toBe(settled);

    // And the session still works: a fresh command behaves exactly as on launch.
    await submitCommand('small');
    const heading = await $('h2=small');
    await heading.waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await blockTextOf('small')) === 'hello from acter', {
      timeout: 10_000,
      timeoutMsg: 'the session was not usable after a stop',
    });
  });
});
