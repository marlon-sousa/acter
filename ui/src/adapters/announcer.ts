// Role: adapter (DOM) — the single polite live region. The region element is created
// once in src/views/main_window.html and never recreated (live-region lifecycle rule).
//
// Announcements are QUEUED and drained one per turn into the live region as distinct
// child nodes. A polite region reads additions in order, and NVDA reads a single
// mutation batch as a single utterance — so two announcements must never share a batch
// (A3.1's "command stopped command stopped" finding). Draining one item per macrotask
// makes each announcement its own mutation, which the screen reader can then speak as a
// separate utterance. The region is never replaced, only appended to and empties.
//
// The region is emptied after a short idle, measured from the last drained announcement,
// so a burst accumulates and clears only once it has settled. Emptying is safe and
// silent: the accessibility event fires on mutation and the screen reader copies the
// text into its own speech queue, so clearing the DOM afterwards cannot retract speech
// that has not been uttered yet, and removals are not announced.
//
// Render-before-announce: `announce` only enqueues; the live region mutates on a later
// turn, so it can never precede the synchronous buffer append that ran before `announce`
// was called. When the buffer later batches appends into `requestAnimationFrame`, this
// adapter gains a commit/acknowledge gate then (recorded in DESIGN's open question), not
// now.
//
// Whether status announcements should live in a SEPARATE region from output (with their
// own interrupt/order semantics) is an open DESIGN question — not decided here.

import type { AnnouncerView } from '../ports/announcer_view';

// How long the region may sit idle (no new drained announcement) before it is emptied.
// Long enough that the accessibility event has certainly been dispatched (that pipeline
// is milliseconds), short enough that stale text is rarely reachable. Tunable by feel
// from the manual NVDA pass.
const CLEAR_AFTER_MS = 1500;

// One announcement is drained per macrotask, so no two announcements ever share a
// live-region mutation batch. The spacing is a small positive duration so a burst's
// chained drains land in distinct timer slots rather than one batched turn. The exact
// spacing at which NVDA treats two additions as separate utterances is measured through
// the screen-reader bridge; if a temporal gap is not enough, the fallback is a
// structural separator (<br>) between successive drains, tuned here.
const DRAIN_SPACING_MS = 1;

export class AnnouncerDom implements AnnouncerView {
  private readonly queue: string[] = [];
  private drainScheduled = false;
  private clearTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly region: HTMLElement) { }

  announce(text: string): void {
    this.queue.push(text);
    this.scheduleDrain();
  }

  private scheduleDrain(): void {
    if (this.drainScheduled) {
      return;
    }
    this.drainScheduled = true;
    setTimeout(() => {
      this.drainScheduled = false;
      this.drainOne();
    }, DRAIN_SPACING_MS);
  }

  private drainOne(): void {
    const text = this.queue.shift();
    if (text === undefined) {
      return;
    }
    const line = document.createElement('div');
    line.textContent = text;
    this.region.append(line);

    // Restart the idle countdown on every drained announcement, so a burst accumulates
    // and is cleared only once it has settled — never out from under its own latest
    // entry.
    clearTimeout(this.clearTimer);
    this.clearTimer = setTimeout(() => {
      this.region.replaceChildren();
    }, CLEAR_AFTER_MS);

    if (this.queue.length > 0) {
      this.scheduleDrain();
    }
  }
}