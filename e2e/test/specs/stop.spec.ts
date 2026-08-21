// Role: e2e spec — stopping a running command with the key the user actually presses.
//
// This file used to submit a typed `stop` line and assert the *silence* B6 left behind:
// Acter had not asked for that interrupt, the far end simply ended the block, and
// claiming "command stopped" would have been a claim about something Acter did not do.
// A3.2 is the entry that comment named. The key now exists, so the whole path runs —
// keyboard adapter, `send_key`, the keybinding policy, `Transport::interrupt`, the far
// end's own interrupt rule, a `D` with no exit code — and this time Acter did the
// stopping, so the pinned phrasing is exactly what must be heard.
//
// The typed `stop` rule is gone from the shipped transcript with this entry, which is
// also why the second case here no longer has to tiptoe around correlation drift.

import { browser, expect } from '@wdio/globals';

import { pressCtrlC, submitCommand } from '../helpers';

// Everything the live region has said, accumulated. The region empties itself on an idle
// timer (A3's browse-mode rule), so sampling `textContent` at the end can miss an
// announcement that has already been cleared; an observer records them as they land.
async function recordAnnouncements(): Promise<void> {
  await browser.execute(() => {
    const target = window as unknown as { __spoken?: string[] };
    target.__spoken = [];
    const announcer = document.getElementById('announcer');
    if (announcer === null) {
      return;
    }
    new MutationObserver(() => {
      const said = announcer.textContent ?? '';
      if (said !== '') {
        target.__spoken?.push(said);
      }
    }).observe(announcer, { childList: true, subtree: true, characterData: true });
  });
}

function spoken(): Promise<string[]> {
  return browser.execute(
    () => (window as unknown as { __spoken?: string[] }).__spoken ?? [],
  );
}

// The text accumulated under a command's h2, or null while no such block exists.
//
// The *most recent* block with that heading: these cases share one app instance, so a
// name submitted more than once has several blocks and only the newest is the live one.
// Reading the oldest would have this report a long-finished command as "not growing".
function blockTextOf(command: string): Promise<string | null> {
  return browser.execute((name: string) => {
    const headings = Array.from(document.querySelectorAll('#results h2'));
    const own = headings.reverse().find((el) => el.textContent === name);
    return own?.nextElementSibling?.textContent ?? null;
  }, command);
}

async function waitUntilRunning(command: string): Promise<void> {
  await browser.waitUntil(
    async () => ((await blockTextOf(command)) ?? '').includes('still working'),
    {
      timeout: 10_000,
      timeoutMsg: `${command} never reached its quiet accumulation loop`,
    },
  );
}

describe('Ctrl+C: stopping a running command', () => {
  it('halts an endless command and says so', async () => {
    await recordAnnouncements();
    await submitCommand('forever');
    // The E2E transcript paces every delivery at 20ms, so the block grows continuously.
    // Wait until it is genuinely running before stopping it.
    await waitUntilRunning('forever');

    await pressCtrlC();

    // Acter asked for this interrupt, so this time the pinned phrasing is owed.
    await browser.waitUntil(
      async () => (await spoken()).some((said) => said.includes('command stopped')),
      { timeout: 10_000, timeoutMsg: 'the stop was never announced' },
    );

    // And the output actually stopped. Sample, wait well past several 20ms intervals,
    // and sample again — a still-running script would have grown.
    await browser.waitUntil(
      async () => {
        const settled = await blockTextOf('forever');
        await browser.pause(500);
        return (await blockTextOf('forever')) === settled;
      },
      { timeout: 10_000, timeoutMsg: 'forever kept producing output after the stop' },
    );
  });

  // The other answer only the frontend can voice, and the one A3.1 decision 6 said the
  // typed `stop` had no honest way to give.
  it('says there is nothing to stop when nothing is running', async () => {
    await recordAnnouncements();

    await pressCtrlC();

    await browser.waitUntil(
      async () =>
        (await spoken()).some((said) => said.includes('nothing running to stop')),
      { timeout: 10_000, timeoutMsg: 'an idle Ctrl+C said nothing at all' },
    );
  });

  // DESIGN layer 2, the half A3.2's NVDA pass forced into words: the interrupt belongs
  // to the edit field and nowhere else. In the results buffer Ctrl+C is the screen
  // reader's own copy command — NVDA answers it in browse mode and it never reaches the
  // page — so a listener binding here would be one that cannot be pressed. The app must
  // not act on it even when it is delivered, which is what this dispatches.
  it('does not stop anything when the key arrives outside the edit field', async () => {
    await recordAnnouncements();
    await submitCommand('forever');
    await waitUntilRunning('forever');

    await browser.execute(() => {
      document.getElementById('results')?.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'c',
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    // Nothing said, and — the decisive half — the command is still producing.
    const before = await blockTextOf('forever');
    await browser.pause(1000);
    expect(await blockTextOf('forever')).not.toBe(before);
    expect(await spoken()).not.toContain('command stopped');

    // And the edit field still can, so this is a rule about where the key lands rather
    // than a session that had already stopped listening.
    await pressCtrlC();
    await browser.waitUntil(
      async () => (await spoken()).some((said) => said.includes('command stopped')),
      { timeout: 10_000, timeoutMsg: 'the edit field could not stop it either' },
    );
  });

  it('leaves the session usable afterwards', async () => {
    await submitCommand('forever');
    await waitUntilRunning('forever');
    await pressCtrlC();

    await submitCommand('small');

    await browser.waitUntil(
      async () => (await blockTextOf('small'))?.includes('hello from acter') ?? false,
      {
        timeout: 10_000,
        timeoutMsg: 'the session did not run a command after a stop',
      },
    );
    // Under its own heading, not the previous command's: nothing minted an id for a
    // keystroke, so there is no queued id for this block to claim by mistake.
    expect(await blockTextOf('small')).toContain('hello from acter');
  });
});
