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
const SPOKEN_MARGIN_MS = 100;

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

  /** **The session's first words are the ones a live region loses.** A region changed while
   * the page is still loading is not announced at all — the reader is still building its
   * view of the document, so the mutation lands before anything is watching. The user met
   * this as an opening prompt that was in the buffer and had never been spoken, while every
   * prompt after it spoke normally (2026-08-25). */
  it('holds the first announcement of a session until the reader could be listening', () => {
    const region = document.createElement('div');
    const announcer = new AnnouncerDom(region, 1_000);

    announcer.announce('where you are');
    vi.advanceTimersByTime(500);
    expect(region.textContent).toBe('');

    vi.advanceTimersByTime(600);
    expect(region.textContent).toBe('where you are');
  });

  /** And it costs nothing afterwards: the hold is the session's opening, not a tax on every
   * announcement. */
  it('does not hold anything after its first', () => {
    const region = document.createElement('div');
    const announcer = new AnnouncerDom(region, 1_000);

    announcer.announce('first');
    vi.advanceTimersByTime(1_100);
    announcer.announce('second');
    vi.advanceTimersByTime(300);

    expect(region.textContent).toContain('second');
  });

  it('does not touch the region until a drain turn; then it lands as a single node', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.announce('hello from acter');

    // Queued, not rendered yet.
    expect(region.childNodes).toHaveLength(0);

    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('costs a lone announcement no waiting: the gap is between announcements, not before one', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

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
    const announcer = new AnnouncerDom(region, 0);

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
    const announcer = new AnnouncerDom(region, 0);

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
    const announcer = new AnnouncerDom(region, 0);
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
    const announcer = new AnnouncerDom(region, 0);

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

/**
 * **A region that has just come back eats the first thing said into it** (roadmap 13.3 and
 * 23.13).
 *
 * While a modal dialog is open the rest of the document is inert; when it closes the region
 * returns to the accessibility tree with no history the reader can compare a change against,
 * and the first change carrying text is lost. Measured across five NVDA passes as a sentence
 * naming the far end that a listener never heard.
 *
 * So a caller that has just closed a dialog says so, and this adapter spends a wordless
 * change on re-establishing the baseline.
 */
describe('coming back from a dialog', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('puts something in the region before the next announcement is made', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    announcer.announce('connected to Command Prompt');

    idleDrainTurn();
    expect(region.children).toHaveLength(1);

    spacedDrainTurn();
    expect(region.children).toHaveLength(2);
    expect(region.children[1]?.textContent).toBe('connected to Command Prompt');
  });

  /** **And it is not an empty node**, which is the whole trick. An empty node is not a text
   * change, so the reader has nothing to take and the announcement after it is still the
   * first one — tried on the real sentence in the real order on 2026-08-30, and it failed. */
  it('carries text, so the reader has a change to lose', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    idleDrainTurn();

    expect(region.children[0]?.textContent).not.toBe('');
  });

  /** **And no words**, or a listener hears it. A full stop worked and was audible — the
   * synthesizer said "ponto" before every connection — so what goes in is a zero-width
   * space, which the same listener heard nothing of. */
  it('says nothing a listener can hear', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    idleDrainTurn();

    expect(region.children[0]?.textContent).toBe('\u200b');
  });

  /** It waits its turn like anything else, so the announcement behind it is not delayed by
   * more than the gap that keeps the two out of one mutation batch. */
  it('is one drain, not a special case', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    announcer.announce('connected to Command Prompt');
    idleDrainTurn();

    vi.advanceTimersByTime(DRAIN_SPACING_MS - 1);
    expect(region.children).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(region.children).toHaveLength(2);
  });
});

/**
 * **Each region is cleared on its own countdown** (roadmap 13.3).
 *
 * One timer could only ever clear one of them, and the drain that starts a countdown is not
 * always the drain that ends it: an announcement into the document's region cancelled the
 * one belonging to a dialog's, so the dialog kept its last words — and spoke them when it
 * next opened. Measured 2026-08-30, where connecting to Command Prompt was greeted with
 * "connected to WSL: Ubuntu, bash".
 */
describe('clearing more than one region', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('does not leave a dialog holding its last words when something else is said', () => {
    const document_region = makeRegion();
    const dialog = document.createElement('dialog');
    dialog.setAttribute('open', '');
    const inside = document.createElement('div');
    inside.setAttribute('data-live-region', '');
    dialog.append(inside);
    document.body.append(dialog);
    const announcer = new AnnouncerDom(document_region, 0);

    // Said into the dialog, which is open.
    announcer.announce('Starting Ubuntu.');
    idleDrainTurn();
    expect(inside.textContent).toBe('Starting Ubuntu.');

    // The dialog closes, and something is said into the document a moment later — which is
    // what used to cancel the dialog's countdown.
    dialog.removeAttribute('open');
    vi.advanceTimersByTime(DRAIN_SPACING_MS);
    announcer.announce('C:\\Users\\marlo>');
    idleDrainTurn();
    expect(document_region.textContent).toBe('C:\\Users\\marlo>');

    // Both are emptied, each on its own countdown, and the dialog has nothing left to say
    // the next time it opens.
    vi.advanceTimersByTime(CLEAR_AFTER_MS * 2);
    expect(inside.textContent).toBe('');
    expect(document_region.textContent).toBe('');
  });
});

/**
 * **Where an announcement goes while dialogs are open.** `showModal` makes everything
 * outside the top dialog inert, so a region under one changes where nothing is listening —
 * which is why a dialog that wants to be heard carries its own.
 *
 * Since 2026-08-30 they stack: the Connect dialog opens a connecting dialog on top of
 * itself, and only the innermost is listened to.
 */
describe('speaking into a dialog', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  function open(id: string): HTMLElement {
    const dialog = document.createElement('dialog');
    dialog.id = id;
    dialog.setAttribute('open', '');
    const region = document.createElement('div');
    region.setAttribute('data-live-region', '');
    dialog.append(region);
    document.body.append(dialog);
    return region;
  }

  it('speaks into an open dialog rather than into the document', () => {
    const document_region = makeRegion();
    const inside = open('connect-dialog');
    const announcer = new AnnouncerDom(document_region, 0);

    announcer.announce('not available');
    vi.advanceTimersByTime(0);

    expect(inside.textContent).toBe('not available');
    expect(document_region.textContent).toBe('');
  });

  /** The innermost one, which is the last written in the document: a dialog is written
   * after the dialog that opens it. */
  it('speaks into the innermost of a stack', () => {
    const document_region = makeRegion();
    const outer = open('connect-dialog');
    const inner = open('connecting-dialog');
    const announcer = new AnnouncerDom(document_region, 0);

    announcer.announce('Starting Ubuntu.');
    vi.advanceTimersByTime(0);

    expect(inner.textContent).toBe('Starting Ubuntu.');
    expect(outer.textContent).toBe('');
  });

  /** And back to the one underneath when the innermost closes, which is what Escape out of
   * the connecting dialog leaves behind. */
  it('speaks into the one underneath once the innermost has closed', () => {
    const document_region = makeRegion();
    const outer = open('connect-dialog');
    const inner = open('connecting-dialog');
    inner.parentElement?.removeAttribute('open');
    const announcer = new AnnouncerDom(document_region, 0);

    announcer.announce('not available');
    vi.advanceTimersByTime(0);

    expect(outer.textContent).toBe('not available');
  });
});
