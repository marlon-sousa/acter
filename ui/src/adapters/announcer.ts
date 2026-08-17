// Role: adapter (DOM) — the single polite live region. The region element is created
// once in src/views/main_window.html and never recreated (live-region lifecycle rule).
//
// Announcements are QUEUED and drained one per turn into the live region as distinct
// child nodes. A polite region reads additions in order, and NVDA reads a single
// mutation batch as a single utterance — so two announcements must never share a batch
// (A3.1's "command stopped command stopped" finding). Draining one item per turn, spaced
// far enough apart to clear the WebView2 accessibility batching window (see
// DRAIN_SPACING_MS), makes each announcement its own live-region change, which the screen
// reader then speaks as a separate utterance. The region is never replaced, only appended
// to and emptied.
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

// How long the drain waits between announcements, so no two ever share a live-region
// mutation batch.
//
// A distinct task is NOT enough on its own. WebView2 batches accessibility updates
// per rendering lifecycle, so mutations closer together than that batch still reach NVDA
// as ONE live-region change and are spoken as one utterance. The spacing therefore has
// to clear the batching window, not merely the JS task queue.
//
// Measured through the screen-reader bridge (NVDA 2026.1.1, WebView2, two commands
// stopped at once, silent capture): 1 ms, 50 ms, and 75 ms all merged into a single
// "command stopped command stopped" utterance; 100 ms and 250 ms produced two separate
// utterances. The threshold on that machine sits between 75 ms and 100 ms, so 100 ms is
// the edge and not a safe setting — the value below is ~2.5x the measured threshold, to
// absorb machine load, WebView2 version, and NVDA cadence. A temporal gap alone was
// sufficient, so the structural-separator (<br>) fallback is deliberately NOT used.
//
// This is a gap BETWEEN announcements, never a delay before one (see scheduleDrain), so
// a lone announcement — the common case — is as prompt as it was before serialization and
// pays nothing. Only the second and later items of a burst wait, which is exactly when
// the wait buys something. Backend pacing (quiescence + babble guard, B1) is already far
// coarser than this gap: in the `burst` scenario the queue never reached depth 2.
const DRAIN_SPACING_MS = 250;

export class AnnouncerDom implements AnnouncerView {
  private readonly queue: string[] = [];
  private drainScheduled = false;
  private lastDrainAt = Number.NEGATIVE_INFINITY;
  private clearTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly region: HTMLElement) {}

  announce(text: string): void {
    this.queue.push(text);
    this.scheduleDrain();
  }

  // The spacing is a gap BETWEEN announcements, not a delay before each one: an
  // announcement arriving into an idle region has nothing to share a mutation batch with,
  // so it drains on the next turn with no added wait. Only an announcement following a
  // recent drain waits, and only for the remainder of the gap. This keeps the common case
  // — one announcement, nothing else pending — as prompt as it was before serialization,
  // while a burst still separates.
  private scheduleDrain(): void {
    if (this.drainScheduled) {
      return;
    }
    this.drainScheduled = true;
    const sinceLastDrain = Date.now() - this.lastDrainAt;
    const wait = Math.max(0, DRAIN_SPACING_MS - sinceLastDrain);
    setTimeout(() => {
      this.drainScheduled = false;
      this.drainOne();
    }, wait);
  }

  private drainOne(): void {
    const text = this.queue.shift();
    if (text === undefined) {
      return;
    }
    const line = document.createElement('div');
    line.textContent = text;
    this.region.append(line);
    this.lastDrainAt = Date.now();

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
