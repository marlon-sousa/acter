// Role: e2e spec — the over-threshold scenario. The `big` rule emits one thirty-line
// chunk, so the live region announces the pinned too-big phrasing with the line count,
// and the output text itself is NOT read aloud.

import { $, browser, expect } from '@wdio/globals';

import { debugTape, submitCommand } from '../helpers';

describe('big: the too-big announcement', () => {
  it('announces the pinned too-big text with the line count', async () => {
    await submitCommand('big');

    const announcer = await $('#announcer');
    await browser.waitUntil(
      async () => (await announcer.getText()).includes('30 lines arrived, too big to read'),
      {
        timeout: 5000,
        timeoutMsg: 'the too-big announcement never reached the live region',
      },
    );

    const announced = await browser.execute(
      () => document.getElementById('announcer')?.textContent ?? '',
    );
    expect(announced).toContain('30 lines arrived, too big to read');
    expect(announced).not.toContain('line 1');
  });

  it('announces the too-big verdict before the command ends, so the beep can arm', async () => {
    await submitCommand('big');

    await browser.waitUntil(
      async () =>
        (await debugTape()).some(
          (entry) => entry.kind === 'event' && entry.what === 'CommandFinished',
        ),
      { timeout: 10_000, timeoutMsg: 'the command never finished' },
    );

    const events = (await debugTape())
      .filter((entry) => entry.kind === 'event')
      .map((entry) => entry.what);
    const verdict = events.indexOf('Announce');
    const ending = events.indexOf('CommandFinished');

    expect(verdict).toBeGreaterThanOrEqual(0);
    expect(ending).toBeGreaterThanOrEqual(0);
    expect(verdict).toBeLessThan(ending);
  });
});
