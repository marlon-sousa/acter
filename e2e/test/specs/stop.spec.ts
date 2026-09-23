// Role: e2e spec — stopping a running command with the key the user actually presses.

import { browser, expect } from '@wdio/globals';

import { pressCtrlC, submitCommand } from '../helpers';

// The region empties itself on an idle timer, so a sample taken at the end can miss an
// announcement; an observer records them as they land.
async function recordAnnouncements(): Promise<void> {
  await browser.execute(() => {
    const target = window as unknown as { __spoken?: string[] };
    target.__spoken = [];
    const announcer = document.getElementById('announcer');
    if (announcer === null) {
      return;
    }
    // Added nodes only: the region's `textContent` can still hold an earlier test's
    // announcements that have not cleared yet.
    new MutationObserver((records) => {
      for (const record of records) {
        for (const added of Array.from(record.addedNodes)) {
          const said = added.textContent ?? '';
          if (said !== '') {
            target.__spoken?.push(said);
          }
        }
      }
    }).observe(announcer, { childList: true, subtree: true, characterData: true });
  });
}

function spoken(): Promise<string[]> {
  return browser.execute(
    () => (window as unknown as { __spoken?: string[] }).__spoken ?? [],
  );
}

// The text under the newest h2 for that command, or null while no such block exists; the
// cases share one app instance, so a name submitted twice has several blocks.
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
  it('halts an endless command, and says nothing of its own about it', async () => {
    await recordAnnouncements();
    await submitCommand('forever');
    await waitUntilRunning('forever');

    await pressCtrlC();

    await browser.waitUntil(
      async () => {
        const settled = await blockTextOf('forever');
        await browser.pause(500);
        return (await blockTextOf('forever')) === settled;
      },
      { timeout: 10_000, timeoutMsg: 'forever kept producing output after the stop' },
    );

    const said = await spoken();
    expect(said.filter((line) => line.includes('command stopped'))).toEqual([]);
  });

  it('says there is nothing to stop when nothing is running', async () => {
    await recordAnnouncements();

    await pressCtrlC();

    await browser.waitUntil(
      async () =>
        (await spoken()).some((said) => said.includes('nothing running to stop')),
      { timeout: 10_000, timeoutMsg: 'an idle Ctrl+C said nothing at all' },
    );
  });

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

    const before = await blockTextOf('forever');
    await browser.pause(1000);
    expect(await blockTextOf('forever')).not.toBe(before);
    const said = await spoken();
    expect(said.filter((line) => line.includes('nothing running to stop'))).toEqual([]);
    expect(said.filter((line) => line.includes('that key does nothing here'))).toEqual([]);

    await pressCtrlC();
    await browser.waitUntil(
      async () => {
        const settled = await blockTextOf('forever');
        await browser.pause(500);
        return (await blockTextOf('forever')) === settled;
      },
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
    expect(await blockTextOf('small')).toContain('hello from acter');
  });
});
