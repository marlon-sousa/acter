// Role: e2e spec — the live-region lifecycle rule, machine-enforced against real DOM
// nodes: after a submit the polite live region holds the response text exactly once,
// and the announcer element is the SAME node it was before the submit (never
// recreated — recreating it would drop pending screen-reader announcements).

import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

describe('announcer: live-region lifecycle', () => {
  it('keeps the same announcer node and announces the response once', async () => {
    // Tag the pre-submit announcer node so we can prove identity across the submit
    // without holding a JS handle the WebDriver bridge cannot preserve.
    await browser.execute(() => {
      const announcer = document.getElementById('announcer');
      (window as unknown as { __announcerNode?: Element | null }).__announcerNode =
        announcer;
    });

    await submitCommand('ping');

    const announcer = await $('#announcer');
    await expect(announcer).toHaveText('ping');

    const state = await browser.execute(() => {
      const current = document.getElementById('announcer');
      const previous = (
        window as unknown as { __announcerNode?: Element | null }
      ).__announcerNode;
      return {
        sameNode: current === previous,
        childCount: current?.childNodes.length ?? -1,
        text: current?.textContent ?? '',
      };
    });

    expect(state.sameNode).toBe(true);
    // Exactly once: a single child holding the response text, not appended twice.
    expect(state.childCount).toBe(1);
    expect(state.text).toBe('ping');
  });
});
