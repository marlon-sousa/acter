// Role: e2e spec - halting an endless scenario. The `forever` rule never ends on its
// own; the transcript's `stop` rule cancels whatever is in flight and closes the block,
// which is what makes an endless script safe to submit at all.
//
// **What it must NOT do is speak.** A3.1 wrote this spec expecting the pinned "command
// stopped" phrasing, and B6's decision 8 took that away on purpose: Acter did not ask
// for this interrupt. The far end simply ended the block with a bare `D`, and a bare `D`
// with nothing outstanding is a command that ended, reported as exit code 0, because
// stranding a session in "running" is the one answer that is certainly wrong. The
// spoken stop belongs to `SessionApi::send_key`, which has a real backend behind it
// since B6 and gets its keystroke in A3.2 - and only then is "command stopped" true,
// because only then did Acter do the stopping.
//
// So this asserts the halt and the silence, and both are the behaviour the spec decided.

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
  it('stops forever without claiming Acter stopped it', async () => {
    await submitCommand('forever');

    // The E2E transcript paces every delivery at 20ms, so the block grows continuously.
    // Wait until it is genuinely running before stopping it.
    await browser.waitUntil(
      async () => ((await blockTextOf('forever')) ?? '').includes('still working'),
      { timeout: 10_000, timeoutMsg: 'forever never reached its quiet accumulation loop' },
    );

    await submitCommand('stop');

    // The decisive assertion: output stopped. Sample, wait well past several 20ms
    // intervals, and sample again - a still-running script would have grown.
    await browser.waitUntil(
      async () => {
        const settled = await blockTextOf('forever');
        await browser.pause(500);
        return (await blockTextOf('forever')) === settled;
      },
      { timeout: 10_000, timeoutMsg: 'forever kept producing output after the stop' },
    );

    // And nothing claimed Acter stopped it. The live region empties on an idle timer,
    // so this reads what is there now rather than what was there at any point; the
    // pinned phrasing would still be present if it had just been announced, since the
    // halt above took well under the clear delay.
    const announced = await browser.execute(
      () => document.getElementById('announcer')?.textContent ?? '',
    );
    expect(announced).not.toContain('command stopped');
  });

  // Deliberately a separate case, on a fresh app instance. Submitting `stop` mints a
  // command id for a line that never opens a block of its own, and B6's decision 3
  // accepts that the next block claims it - so after a stop, block headings in THIS
  // session are one command behind. Asserting "the session is still usable" in the same
  // case would therefore be asserting the drift, which roadmap entry B6.1 exists to
  // remove. What is genuinely true, and is what this checks, is that the session still
  // runs commands and their output still reaches the buffer.
  it('still runs commands after an endless one was halted', async () => {
    await submitCommand('forever');
    await browser.waitUntil(
      async () => ((await blockTextOf('forever')) ?? '').includes('still working'),
      { timeout: 10_000, timeoutMsg: 'forever never reached its quiet accumulation loop' },
    );
    await submitCommand('stop');
    await submitCommand('small');

    await browser.waitUntil(
      async () => {
        const results = await browser.execute(
          () => document.getElementById('results')?.textContent ?? '',
        );
        return results.includes('hello from acter');
      },
      { timeout: 10_000, timeoutMsg: 'the session did not run a command after a stop' },
    );
  });
});
