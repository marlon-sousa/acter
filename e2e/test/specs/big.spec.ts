// Role: e2e spec — the over-threshold scenario. The `big` rule emits one thirty-line
// chunk, so the live region announces the pinned too-big phrasing with the line count,
// and the output text itself is NOT read aloud.
//
// It also pins the *order* the completion beep depends on. WebDriver still cannot hear,
// but the beep's arming is not an audible fact: the frontend fires it on the ending event
// for any command a `TooBig` armed, so if the verdict arrives after the ending it can
// never fire. That is exactly what happened until A3.2 — no too-big command had ever
// beeped, and nothing visible said so, because the defect lived in the order rather than
// in the audio. The debug recorder makes that order readable, which is what it is for.
//
// Thirty is the transcript's number, and since B6 the count in the phrasing is the real
// policy's: the actor measures the text against `PacingConfig`'s threshold and sends
// `Announce { TooBig { lines } }`. Nothing here is scripted any more, which is why this
// assertion is worth making at all.

import { $, browser, expect } from '@wdio/globals';

import { debugTape, submitCommand } from '../helpers';

describe('big: the too-big announcement', () => {
  it('announces the pinned too-big text with the line count', async () => {
    await submitCommand('big');

    const announcer = await $('#announcer');
    // Pinned string (spec decision 3); the transcript's `big` rule emits thirty lines.
    // Since B5.6 a marked session also announces the prompt the shell drew, so the live
    // region legitimately holds more than one utterance during a burst: it is a queue
    // drained one child per turn, not a single slot. What this test is about is that the
    // phrase was announced, so it asks whether the region contains it rather than whether
    // the region is only it.
    await browser.waitUntil(
      async () => (await announcer.getText()).includes('30 lines arrived, too big to read'),
      {
        timeout: 5000,
        timeoutMsg: 'the too-big announcement never reached the live region',
      },
    );

    // The thirty lines are in the buffer but were not spoken. What the region must hold is
    // the too-big phrasing and none of the output — it may also hold the prompt the shell
    // drew, which since B5.6 is announced beside it and is not output either.
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
    // Reversed, the frontend arms the beep for a command it has already finished with,
    // and no too-big command ever beeps.
    expect(verdict).toBeLessThan(ending);
  });
});
