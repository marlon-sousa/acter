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
//
// **A modal dialog needs a live region of its own, and this adapter finds it.** `showModal`
// puts the dialog in the top layer and makes the rest of the document inert — everything
// outside it leaves the accessibility tree — so the region at the end of `<body>` changes
// where nothing is listening. Measured with NVDA 2026.1.1 on 2026-08-26, driving the
// Connect dialog: arrowing its list announced each kind and said nothing at all about the
// panel, which is precisely the silent-change trap that announcement exists to prevent.
// Any dialog that wants to be heard therefore carries `data-live-region`, and drains go
// there while it is open.

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

// How long the first announcement of a session waits before it is put into the region.
//
// **A live region changed while the page is still loading is not announced.** The reader is
// still building its view of the document when the change happens, so the mutation lands
// before there is anything watching for it — and the announcement is not late, it is gone.
// This is not theoretical: the user met it on 2026-08-25 as a session whose opening prompt
// was in the buffer and had never been spoken, while every prompt after it spoke normally.
// It is the session's *first* words that are lost, which are the ones telling a listener
// where they are.
//
// The hold applies once, to the first drain of the session, so nothing afterwards pays for
// it. The value is a starting point rather than a measurement: unlike DRAIN_SPACING_MS,
// which was measured against a real reader, nobody has yet found where the threshold sits
// here. It should be tuned by ear and this comment corrected when it is.
const STARTUP_HOLD_MS = 1_000;

export class AnnouncerDom implements AnnouncerView {
  private readonly queue: string[] = [];
  private drainScheduled = false;
  private lastDrainAt = Number.NEGATIVE_INFINITY;
  private clearTimer: ReturnType<typeof setTimeout> | undefined;
  /** The earliest moment anything may be put into the region. See STARTUP_HOLD_MS. */
  private readonly openAt: number;

  /**
   * `startupHold` is a parameter rather than a constant read directly, so a test can say
   * "no hold" and go on asserting the timing rules that were measured against a real
   * reader — chief among them that a lone announcement waits for nothing. Production
   * passes nothing and gets the hold.
   */
  constructor(
    private readonly region: HTMLElement,
    startupHold: number = STARTUP_HOLD_MS,
  ) {
    this.openAt = Date.now() + startupHold;
  }

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
    // Whichever is further away: the gap after the previous announcement, or the hold that
    // keeps the session's first words out of a region nothing is watching yet.
    const wait = Math.max(
      0,
      DRAIN_SPACING_MS - sinceLastDrain,
      this.openAt - Date.now(),
    );
    setTimeout(() => {
      this.drainScheduled = false;
      this.drainOne();
    }, wait);
  }

  /**
   * Where an announcement goes right now: an open dialog's own region if there is one,
   * and the document's otherwise.
   *
   * Found by attribute rather than by name, so this adapter knows that dialogs exist and
   * nothing about which ones — a second dialog that needs to be heard adds the attribute
   * and needs no change here.
   */
  private liveRegion(): HTMLElement {
    return (
      this.region.ownerDocument.querySelector<HTMLElement>(
        'dialog[open] [data-live-region]',
      ) ?? this.region
    );
  }

  private drainOne(): void {
    const text = this.queue.shift();
    if (text === undefined) {
      return;
    }
    const region = this.liveRegion();
    const line = document.createElement('div');
    line.textContent = text;
    region.append(line);
    this.lastDrainAt = Date.now();

    // Restart the idle countdown on every drained announcement, so a burst accumulates
    // and is cleared only once it has settled — never out from under its own latest
    // entry. The region cleared is the one written to, not whichever is current when the
    // timer fires: a dialog that closes in between must not leave its last words behind.
    clearTimeout(this.clearTimer);
    this.clearTimer = setTimeout(() => {
      region.replaceChildren();
    }, CLEAR_AFTER_MS);

    if (this.queue.length > 0) {
      this.scheduleDrain();
    }
  }
}
