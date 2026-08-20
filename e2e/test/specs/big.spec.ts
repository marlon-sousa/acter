// Role: e2e spec — the over-threshold scenario. The `big` rule emits one thirty-line
// chunk, so the live region announces the pinned too-big phrasing with the line count,
// and the output text itself is NOT read aloud. (The completion beep is manual-checklist
// territory — WebDriver cannot hear.)
//
// Thirty is the transcript's number, and since B6 the count in the phrasing is the real
// policy's: the actor measures the text against `PacingConfig`'s threshold and sends
// `Announce { TooBig { lines } }`. Nothing here is scripted any more, which is why this
// assertion is worth making at all.

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('big: the too-big announcement', () => {
  it('announces the pinned too-big text with the line count', async () => {
    await submitCommand('big');

    const announcer = await $('#announcer');
    // Pinned string (spec decision 3); the transcript's `big` rule emits thirty lines.
    await expect(announcer).toHaveText('30 lines arrived, too big to read');

    // The thirty lines are in the buffer but were not spoken: the live region holds the
    // too-big phrasing, not the output text.
    const announced = await browser.execute(
      () => document.getElementById('announcer')?.textContent ?? '',
    );
    expect(announced).toBe('30 lines arrived, too big to read');
    expect(announced).not.toContain('line 1');
  });
});
