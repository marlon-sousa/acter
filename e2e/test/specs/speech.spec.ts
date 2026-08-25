// Role: e2e spec — the `speech` scenario emits one long auto-read phrase with an
// unmistakable closing marker ("long announcement finished"). It exists mainly for the
// manual NVDA pass (does emptying the live region truncate queued speech?), but the DOM
// half is checkable here: the whole phrase, marker included, is placed into the region
// as a single announcement — the clearing that follows removes it silently, it never
// truncates what was placed.

import { browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('speech: a long single-utterance announcement', () => {
  it('places the whole phrase including the closing marker into the live region', async () => {
    await submitCommand('speech');

    // The entire phrase lands as one announcement; assert the closing marker is
    // present, proving nothing was truncated on the way into the region.
    const text = (await browser.waitUntil(
      async () => {
        const current = await browser.execute(
          () => document.getElementById('announcer')?.textContent ?? '',
        );
        return current.includes('long announcement finished') ? current : false;
      },
      { timeout: 5000, timeoutMsg: 'the long announcement never appeared in full' },
    )) as string;

    // The whole phrase reached the region as one utterance — asserted by finding it
    // entire rather than by owning both ends of the region, which stopped being true when
    // B5.6 gave a marked session a prompt to announce beside it.
    expect(text).toContain('long announcement starting.');
    expect(text).toContain('long announcement finished');
  });
});
