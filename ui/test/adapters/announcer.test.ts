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

  it('announces the text as a single node', () => {
    const region = makeRegion();
    new AnnouncerDom(region).announce('hello from acter');

    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('appends back-to-back announcements as distinct nodes so neither clobbers the other', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    // The fail case: error output immediately followed by the failure status.
    announcer.announce('error: the command reported a problem');
    announcer.announce('command failed, exit code 2');

    // Both are present, in order, for the AT to read as two additions.
    expect(region.childNodes).toHaveLength(2);
    expect(region.children[0]?.textContent).toBe(
      'error: the command reported a problem',
    );
    expect(region.children[1]?.textContent).toBe('command failed, exit code 2');
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

  it('restarts the idle countdown on each announcement so a burst is never cut short', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('phase one');
    vi.advanceTimersByTime(CLEAR_AFTER_MS - 100);
    announcer.announce('phase two');

    // The first announcement's timer must not clear the accumulated burst.
    vi.advanceTimersByTime(200);
    expect(region.childNodes).toHaveLength(2);
    expect(region.textContent).toBe('phase onephase two');

    // After the last announcement's own idle window, the whole burst is cleared.
    vi.advanceTimersByTime(CLEAR_AFTER_MS);
    expect(region.childNodes).toHaveLength(0);
    expect(region.textContent).toBe('');
  });
});
