// @vitest-environment jsdom
// Role: test — AnnouncerDom live-region lifecycle in a real DOM.

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

function idleDrainTurn(): void {
  vi.advanceTimersByTime(0);
}

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

  it('holds the first announcement of a session until the reader could be listening', () => {
    const region = document.createElement('div');
    const announcer = new AnnouncerDom(region, 1_000);

    announcer.announce('where you are');
    vi.advanceTimersByTime(500);
    expect(region.textContent).toBe('');

    vi.advanceTimersByTime(600);
    expect(region.textContent).toBe('where you are');
  });

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

    expect(region.childNodes).toHaveLength(0);

    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.textContent).toBe('hello from acter');
  });

  it('costs a lone announcement no waiting: the gap is between announcements, not before one', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.announce('hello from acter');
    idleDrainTurn();
    expect(region.textContent).toBe('hello from acter');

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

    vi.advanceTimersByTime(10);
    announcer.announce('command failed, exit code 2');
    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);

    vi.advanceTimersByTime(DRAIN_SPACING_MS - 10);
    expect(region.childNodes).toHaveLength(2);
    expect(region.children[1]?.textContent).toBe('command failed, exit code 2');
  });

  it('drains back-to-back announcements in separate turns so neither shares a mutation batch', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.announce('error: the command reported a problem');
    announcer.announce('command failed, exit code 2');

    idleDrainTurn();
    expect(region.childNodes).toHaveLength(1);
    expect(region.children[0]?.textContent).toBe(
      'error: the command reported a problem',
    );

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

    vi.advanceTimersByTime(CLEAR_AFTER_MS - 1);
    expect(region.textContent).toBe('hello from acter');

    vi.advanceTimersByTime(1);
    expect(region.textContent).toBe('');
    expect(region.childNodes).toHaveLength(0);
    expect(document.getElementById('announcer')).toBe(region);
  });

  it('restarts the idle countdown on each drained announcement so a burst is never cut short', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.announce('phase one');
    idleDrainTurn();

    // The drain, not the announce, restarts the countdown.
    vi.advanceTimersByTime(CLEAR_AFTER_MS / 2);
    announcer.announce('phase two');
    spacedDrainTurn();

    vi.advanceTimersByTime(CLEAR_AFTER_MS / 2 + DRAIN_SPACING_MS);
    expect(region.childNodes).toHaveLength(2);
    expect(region.textContent).toBe('phase onephase two');

    vi.advanceTimersByTime(CLEAR_AFTER_MS);
    expect(region.childNodes).toHaveLength(0);
    expect(region.textContent).toBe('');
  });
});

describe('knowing when everything has been said', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it('resolves at once when nothing has been said', async () => {
    const announcer = new AnnouncerDom(makeRegion(), 0);

    const settled = watch(announcer.drained());
    await vi.advanceTimersByTimeAsync(0);

    expect(settled()).toBe(true);
  });

  it('waits until the queue is empty', async () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);
    announcer.announce('connected to the far end');
    announcer.announce('who has the keys');

    const settled = watch(announcer.drained());
    await vi.advanceTimersByTimeAsync(0);
    expect(settled()).toBe(false);

    await vi.advanceTimersByTimeAsync(DRAIN_SPACING_MS);
    expect(region.children).toHaveLength(2);
    expect(settled()).toBe(false);
  });

  it('gives the last announcement the same gap a second one would have given it', async () => {
    const announcer = new AnnouncerDom(makeRegion(), 0);
    announcer.announce('the only thing said');

    const settled = watch(announcer.drained());
    await vi.advanceTimersByTimeAsync(0);
    expect(settled()).toBe(false);

    await vi.advanceTimersByTimeAsync(DRAIN_SPACING_MS);
    expect(settled()).toBe(true);
  });
});

function watch(waiting: Promise<void>): () => boolean {
  let done = false;
  void waiting.then(() => {
    done = true;
  });
  return () => done;
}

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

  it('carries text, so the reader has a change to lose', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    idleDrainTurn();

    expect(region.children[0]?.textContent).not.toBe('');
  });

  it('says nothing a listener can hear', () => {
    const region = makeRegion();
    const announcer = new AnnouncerDom(region, 0);

    announcer.documentReturned();
    idleDrainTurn();

    expect(region.children[0]?.textContent).toBe('\u200b');
  });

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

    announcer.announce('Starting Ubuntu.');
    idleDrainTurn();
    expect(inside.textContent).toBe('Starting Ubuntu.');

    dialog.removeAttribute('open');
    vi.advanceTimersByTime(DRAIN_SPACING_MS);
    announcer.announce('C:\\Users\\marlo>');
    idleDrainTurn();
    expect(document_region.textContent).toBe('C:\\Users\\marlo>');

    vi.advanceTimersByTime(CLEAR_AFTER_MS * 2);
    expect(inside.textContent).toBe('');
    expect(document_region.textContent).toBe('');
  });
});

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
