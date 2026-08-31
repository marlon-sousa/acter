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
//
// **A region that has just come back eats the first thing said into it.** When the dialog
// above closes, its document returns to the accessibility tree with no history the reader can
// compare a change against, and the first change carrying text is lost — which is how the
// sentence naming the far end a connection reached went missing on six occasions across five
// NVDA passes (roadmap 13.3 and 23.13). `documentReturned` is how a caller says the region
// is back, so this adapter can spend a wordless change on re-establishing that baseline; see
// SILENT_MARKER for what it is and why it is not empty.

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

// What is put into the region to re-establish its baseline, so the announcement after it is
// not the first change and is therefore not the one that is lost.
//
// **It has to carry text, and it has to say nothing.** Those pull against each other, and
// both halves were measured on 2026-08-30 through the screen-reader bridge (NVDA 2026.1.1):
//
// - An EMPTY node does not work, because an empty node is not a text change at all: tried on
//   the real sentence in the real order, the sentence was still lost. Do not revive it.
// - A full stop works — ten connections out of ten spoke their sentence with one in front —
//   and is AUDIBLE: in live capture the synthesizer said "ponto" before every connection,
//   which is this machine's punctuation level doing exactly what it should.
//
// A zero-width space is the one that is both: five connections out of five spoke their
// sentence, and the listener at the keyboard reported hearing nothing at all before it.
const SILENT_MARKER = '\u200b';

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
  /**
   * The idle countdown for each region that has been written to, kept per region.
   *
   * **One timer could only ever clear one of them**, and the drain that started it is not
   * always the drain that ends it: an announcement into the document's region cancelled the
   * countdown belonging to a dialog's, so the dialog kept its last words for the rest of the
   * session — and spoke them when it next opened. Measured 2026-08-30, where a connection to
   * Command Prompt was greeted with "connected to WSL: Ubuntu, bash".
   */
  private readonly clearTimers = new Map<
    HTMLElement,
    ReturnType<typeof setTimeout>
  >();
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

  /**
   * Spend a wordless change on the region, so the next announcement is not the first one
   * after the document came back — and is therefore not the one that is lost.
   *
   * It goes through the queue like anything else, which is what makes it work: the drain
   * spacing keeps it out of the next announcement's mutation batch, so the reader sees two
   * changes rather than one. A caller that closes a dialog and then announces something needs
   * no other knowledge than that it closed a dialog.
   */
  documentReturned(): void {
    this.announce(SILENT_MARKER);
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
   *
   * **The last one, because dialogs stack.** Since 2026-08-30 the Connect dialog opens a
   * connecting dialog on top of itself, and only the innermost of a stack is listened to:
   * everything under it is inert, so a region there changes where nothing is watching —
   * which is the very thing this method exists to avoid. The innermost is the one written
   * last in the document, because a dialog is written after the dialog that opens it, and
   * `querySelectorAll` answers in document order.
   */
  private liveRegion(): HTMLElement {
    const regions = this.region.ownerDocument.querySelectorAll<HTMLElement>(
      'dialog[open] [data-live-region]',
    );
    return regions[regions.length - 1] ?? this.region;
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

    // Restart the idle countdown for THIS region, so a burst accumulates and is cleared only
    // once it has settled — never out from under its own latest entry, and never because
    // something was said somewhere else (see clearTimers).
    clearTimeout(this.clearTimers.get(region));
    this.clearTimers.set(
      region,
      setTimeout(() => {
        region.replaceChildren();
        this.clearTimers.delete(region);
      }, CLEAR_AFTER_MS),
    );

    if (this.queue.length > 0) {
      this.scheduleDrain();
    }
  }
}
