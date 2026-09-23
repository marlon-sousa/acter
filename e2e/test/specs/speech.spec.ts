// Role: e2e spec — the `speech` scenario's long phrase, closing marker included, lands in the
// live region as one announcement.

import { browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('speech: a long single-utterance announcement', () => {
  it('places the whole phrase including the closing marker into the live region', async () => {
    await submitCommand('speech');

    const text = (await browser.waitUntil(
      async () => {
        const current = await browser.execute(
          () => document.getElementById('announcer')?.textContent ?? '',
        );
        return current.includes('long announcement finished') ? current : false;
      },
      { timeout: 5000, timeoutMsg: 'the long announcement never appeared in full' },
    )) as string;

    expect(text).toContain('long announcement starting.');
    expect(text).toContain('long announcement finished');
  });
});
