// Role: adapter (DOM) — the single polite live region. The region element is created
// once in src/views/main_window.html and never recreated (live-region lifecycle rule).
//
// Announcements are APPENDED as distinct child nodes, not replaced. A polite region
// reads additions in order (default aria-relevant is "additions text"), so two
// announcements arriving back-to-back — e.g. a command's error output immediately
// followed by "command failed, exit code N" — are both spoken, in order, instead of
// the second clobbering the first. Throttling a genuine flood is not this layer's job:
// the backend pacing policy (quiescence + babble guard, B1) decides what becomes an
// announcement, so by the time text reaches here it is worth saying and the frontend
// says all of it. Whether status announcements should live in a SEPARATE region from
// output (with their own interrupt/order semantics) is an open DESIGN question tied to
// that pacing work — not decided here.
//
// The region is emptied after a short idle. A live region must stay in the
// accessibility tree to be announced, so its text is also reachable by browse-mode
// navigation; emptying it once announcements settle keeps stale text out of the browse
// buffer. Emptying is safe and silent: the accessibility event fires on mutation and
// the screen reader copies the text into its own speech queue, so clearing the DOM
// afterwards cannot retract speech that has not been uttered yet, and removals are not
// announced.

import type { AnnouncerView } from '../ports/announcer_view';

// How long the region may sit idle (no new announcement) before it is emptied. Long
// enough that the accessibility event has certainly been dispatched (that pipeline is
// milliseconds), short enough that stale text is rarely reachable. Tunable by feel from
// the manual NVDA pass.
const CLEAR_AFTER_MS = 1500;

export class AnnouncerDom implements AnnouncerView {
  private clearTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly region: HTMLElement) {}

  announce(text: string): void {
    const line = document.createElement('div');
    line.textContent = text;
    this.region.append(line);

    // Restart the idle countdown on every announcement, so a burst accumulates and is
    // cleared only once it has settled — never out from under its own latest entry.
    clearTimeout(this.clearTimer);
    this.clearTimer = setTimeout(() => {
      this.region.replaceChildren();
    }, CLEAR_AFTER_MS);
  }
}
