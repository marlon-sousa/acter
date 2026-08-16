// @vitest-environment jsdom
// Role: test — AnnouncerDom live-region lifecycle in a real DOM: announcements are
// queued and drained one per turn so no two share a mutation batch, the region empties
// afterwards so stale text is never reachable in browse mode, and the region node is
// never recreated (recreating it would drop pending screen-reader announcements).

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnnouncerDom } from '../../src/adapters/announcer';

// Must match the adapter constants; the tests advance past them.
const CLEAR_AFTER_MS = 1500;
const DRAIN_SPACING_MS = 1;

function makeRegion(): HTMLElement {
  const region = document.createElement('div');
  region.id = 'announcer';
  region.setAttribute('aria-live', 'polite');
  document.body.append(region);
  return region;
}

/** Advance exactly one drain turn (the spacing), firing a single chained drain. */
function drainTurn(): void {
  vi.advanceTimersByTime(DRAIN_SPACING_MS);
}

describe('AnnouncerDom', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('does not touch the region until a drain turn; then it lands as a single node', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('hello from acter');

    // Queued, not rendered yet.
    expect(region.childNodes).toHaveLength(0);

    drainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('drains back-to-back announcements in separate turns so neither shares a mutation batch', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('error: the command reported a problem');
    announcer.announce('command failed, exit code 2');

    // First turn drains exactly one announcement — the second must still be queued.
    drainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.children[0]?.textContent).toBe(
      'error: the command reported a problem',
    );

    drainTurn();
    expect(region.childNodes).toHaveLength(2);
    expect(region.children[1]?.textContent).toBe('command failed, exit code 2');
  });

  it('empties the region after the clear delay without replacing the node', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);
    announcer.announce('hello from acter');
    drainTurn();

    // Still present right up to the delay: the announcement must not be clipped.
    vi.advanceTimersByTime(CLEAR_AFTER_MS - 1);
    expect(region.textContent).toBe('hello from acter');

    vi.advanceTimersByTime(1);
    expect(region.textContent).toBe('');
    expect(region.childNodes).toHaveLength(0);
    // The same node the whole time — the live-region lifecycle rule.
    expect(document.getElementById('announcer')).toBe(region);
  });

  it('restarts the idle countdown on each drained announcement so a burst is never cut short', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('phase one');
    drainTurn();
    vi.advanceTimersByTime(CLEAR_AFTER_MS - 100);
    announcer.announce('phase two');
    drainTurn();

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