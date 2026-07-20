// @vitest-environment jsdom
// Role: test — AnnouncerDom live-region lifecycle in a real DOM: announce once, empty
// afterwards so stale text is never reachable in browse mode, and never recreate the
// region node (recreating it would drop pending screen-reader announcements).

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnnouncerDom } from '../../src/adapters/announcer';

// Must match CLEAR_AFTER_MS in the adapter; the tests advance past it.
const CLEAR_AFTER_MS = 1500;

function makeRegion(): HTMLElement {
  const region = document.createElement('div');
  region.id = 'announcer';
  region.setAttribute('aria-live', 'polite');
  document.body.append(region);
  return region;
}

describe('AnnouncerDom', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('announces the text exactly once', () => {
    const region = makeRegion();
    new AnnouncerDom(region).announce('hello from acter');

    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('empties the region after the clear delay without replacing the node', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);
    announcer.announce('hello from acter');

    // Still present right up to the delay: the announcement must not be clipped.
    vi.advanceTimersByTime(CLEAR_AFTER_MS - 1);
    expect(region.textContent).toBe('hello from acter');

    vi.advanceTimersByTime(1);
    expect(region.textContent).toBe('');
    expect(region.childNodes).toHaveLength(0);
    // The same node the whole time — the live-region lifecycle rule.
    expect(document.getElementById('announcer')).toBe(region);
  });

  it('restarts the countdown on each announcement so a burst is never cut short', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('phase one');
    vi.advanceTimersByTime(CLEAR_AFTER_MS - 100);
    announcer.announce('phase two');

    // The first announcement's timer must not clear the second one.
    vi.advanceTimersByTime(200);
    expect(region.textContent).toBe('phase two');

    // The second announcement's own countdown still applies.
    vi.advanceTimersByTime(CLEAR_AFTER_MS);
    expect(region.textContent).toBe('');
  });

  it('replaces previous text rather than appending it', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('first');
    announcer.announce('second');

    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('second');
  });
});
