// Role: e2e spec — the live-region lifecycle rule against real DOM nodes.

import { browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

interface AnnouncerState {
  sameNode: boolean;
  childCount: number;
  text: string;
}

function snapshotAnnouncer(): AnnouncerState {
  const current = document.getElementById('announcer');
  const previous = (window as unknown as { __announcerNode?: Element | null })
    .__announcerNode;
  return {
    sameNode: current === previous,
    childCount: current?.childNodes.length ?? -1,
    text: current?.textContent ?? '',
  };
}

describe('announcer: live-region lifecycle', () => {
  it('announces once on the same node, then empties itself', async () => {
    // Tagged rather than held, because the WebDriver bridge cannot preserve a JS element
    // handle across the submit.
    await browser.execute(() => {
      const announcer = document.getElementById('announcer');
      (window as unknown as { __announcerNode?: Element | null }).__announcerNode =
        announcer;
    });

    await submitCommand('small');

    const announced = (await browser.waitUntil(
      async () => {
        const snap = await browser.execute(snapshotAnnouncer);
        return snap.text.includes('hello from acter') ? snap : false;
      },
      { timeout: 5000, timeoutMsg: 'the auto-read announcement never appeared' },
    )) as AnnouncerState;

    expect(announced.sameNode).toBe(true);
    expect(announced.text.split('hello from acter').length - 1).toBe(1);

    await browser.waitUntil(
      async () => {
        const snap = await browser.execute(snapshotAnnouncer);
        return snap.sameNode && snap.childCount === 0 && snap.text === '';
      },
      { timeout: 5000, timeoutMsg: 'the announcer never cleared after its delay' },
    );
  });
});
