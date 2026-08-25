// Role: e2e spec — the live-region lifecycle rule, machine-enforced against real DOM
// nodes: after the auto-read scenario the polite live region holds the output text
// exactly once, the announcer element is the SAME node it was before (never recreated
// — recreating it would drop pending screen-reader announcements), and it EMPTIES
// itself shortly afterward so stale announcement text is not left reachable in
// browse-mode navigation.

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
    // Tag the pre-submit announcer node so we can prove identity across the submit
    // without holding a JS handle the WebDriver bridge cannot preserve.
    await browser.execute(() => {
      const announcer = document.getElementById('announcer');
      (window as unknown as { __announcerNode?: Element | null }).__announcerNode =
        announcer;
    });

    // Since B5.6 a marked session also announces the prompt the shell drew, so the live
    // region legitimately holds more than one utterance during a burst: it is a queue
    // drained one child per turn, not a single slot. What this test is about is that the
    // phrase was announced, so it asks whether the region contains it rather than whether
    // the region is only it.
    await submitCommand('small');

    // Catch the announced state the moment the text lands, before the clear delay.
    const announced = (await browser.waitUntil(
      async () => {
        const snap = await browser.execute(snapshotAnnouncer);
        return snap.text.includes('hello from acter') ? snap : false;
      },
      { timeout: 5000, timeoutMsg: 'the auto-read announcement never appeared' },
    )) as AnnouncerState;

    expect(announced.sameNode).toBe(true);
    // Not appended twice: the output text appears once however many other announcements
    // the burst carried. Counting children stopped saying that when B5.6 gave a marked
    // session a prompt to announce as well, so the assertion is about the text.
    expect(announced.text.split('hello from acter').length - 1).toBe(1);

    // Then it empties itself without recreating the node — so a screen reader
    // arrowing past the region later finds nothing stale to read.
    await browser.waitUntil(
      async () => {
        const snap = await browser.execute(snapshotAnnouncer);
        return snap.sameNode && snap.childCount === 0 && snap.text === '';
      },
      { timeout: 5000, timeoutMsg: 'the announcer never cleared after its delay' },
    );
  });
});
