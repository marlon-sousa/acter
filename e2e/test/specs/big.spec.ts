// Role: e2e spec — the over-threshold scenario. The `big` script emits one 40-line
// too-big chunk, so the live region announces the pinned too-big phrasing with the
// line count, and the output text itself is NOT read aloud. (The completion beep is
// manual-checklist territory — WebDriver cannot hear.)

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('big: the too-big announcement', () => {
  it('announces the pinned too-big text with the line count', async () => {
    await submitCommand('big');

    const announcer = await $('#announcer');
    // Pinned string (spec decision 3); the E2E config sets big.line_count to 40.
    await expect(announcer).toHaveText('40 lines arrived, too big to read');

    // The 40 lines are in the buffer but were not spoken: the live region holds the
    // too-big phrasing, not the output text.
    const announced = await browser.execute(
      () => document.getElementById('announcer')?.textContent ?? '',
    );
    expect(announced).toBe('40 lines arrived, too big to read');
    expect(announced).not.toContain('line 1');
  });
});
