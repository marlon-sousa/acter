// @vitest-environment jsdom
// Role: test — AnnouncerDom live-region lifecycle in a real DOM: announcements are
// queued and drained one per turn so no two share a mutation batch, the region empties
// afterwards so stale text is never reachable in browse mode, and the region node is
// never recreated (recreating it would drop pending screen-reader announcements).

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnnouncerDom } from '../../src/adapters/announcer';

// Must match the adapter constants; the tests advance past them.
const CLEAR_AFTER_MS = 1500;
const DRAIN_SPACING_MS = 250;

function makeRegion(): HTMLElement {
  const region = document.createElement('div');
  region.id = 'announcer';
  region.setAttribute('aria-live', 'polite');
  document.body.append(region);
  return region;
}

/**
 * Advance the turn that drains an announcement arriving into an idle region. It waits
 * for nothing but the deferral itself, so no time has to pass.
 */
function idleDrainTurn(): void {
  vi.advanceTimersByTime(0);
}

/**
 * Advance the turn that drains an announcement queued behind a recent drain: it waits
 * out the gap that keeps the two out of one mutation batch.
 */
function spacedDrainTurn(): void {
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

    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('costs a lone announcement no waiting: the gap is between announcements, not before one', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    // Nothing has been drained, so this announcement cannot share a mutation batch with
    // anything and must not be made to wait out the spacing.
    announcer.announce('hello from acter');
    idleDrainTurn();
    expect(region.textContent).toBe('hello from acter');

    // Same again once the region has gone quiet for longer than the gap: the next lone
    // announcement is still prompt, because the gap has already elapsed.
    vi.advanceTimersByTime(DRAIN_SPACING_MS * 4);
    announcer.announce('and again');
    idleDrainTurn();
    expect(region.children[1]?.textContent).toBe('and again');
  });

  it('still spaces an announcement arriving just after a drain, though the queue is empty', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('error: the command reported a problem');
    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);

    // An empty queue is NOT what earns an immediate drain — an elapsed gap is. This
    // announcement finds the queue empty, because the previous one has already drained,
    // yet it arrives close enough behind that drain to land in the same mutation batch.
    // Draining it on the spot is exactly how two announcements merge into one utterance.
    vi.advanceTimersByTime(10);
    announcer.announce('command failed, exit code 2');
    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);

    // It goes out once the remainder of the gap has passed — no longer than that.
    vi.advanceTimersByTime(DRAIN_SPACING_MS - 10);
    expect(region.childNodes).toHaveLength(2);
    expect(region.children[1]?.textContent).toBe('command failed, exit code 2');
  });

  it('drains back-to-back announcements in separate turns so neither shares a mutation batch', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);

    announcer.announce('error: the command reported a problem');
    announcer.announce('command failed, exit code 2');

    // The first drains at once; the second must still be queued, held back by the gap.
    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.children[0]?.textContent).toBe(
      'error: the command reported a problem',
    );

    // It is still queued right up to the end of the gap — that wait is the whole point.
    vi.advanceTimersByTime(DRAIN_SPACING_MS - 1);
    expect(region.childNodes).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(region.childNodes).toHaveLength(2);
    expect(region.children[1]?.textContent).toBe('command failed, exit code 2');
  });

  it('empties the region after the clear delay without replacing the node', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region);
    announcer.announce('hello from acter');
    idleDrainTurn();

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

    // The first item drains, starting a clear countdown measured from that drain.
    announcer.announce('phase one');
    idleDrainTurn();

    // The second item drains comfortably inside that countdown, so it restarts it. The
    // announce has to be early enough that its own drain turn lands inside too — the
    // drain, not the announce, is what restarts the countdown.
    vi.advanceTimersByTime(CLEAR_AFTER_MS / 2);
    announcer.announce('phase two');
    spacedDrainTurn();

    // Past the moment the FIRST drain's countdown would have fired, the burst is still
    // intact: the second drain restarted it.
    vi.advanceTimersByTime(CLEAR_AFTER_MS / 2 + DRAIN_SPACING_MS);
    expect(region.childNodes).toHaveLength(2);
    expect(region.textContent).toBe('phase onephase two');

    // After the last announcement's own idle window, the whole burst is cleared.
    vi.advanceTimersByTime(CLEAR_AFTER_MS);
    expect(region.childNodes).toHaveLength(0);
    expect(region.textContent).toBe('');
  });
});
