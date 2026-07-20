// Role: adapter (DOM) — the single polite live region. The region element is created
// once in src/views/main_window.html and never recreated (live-region lifecycle rule);
// announcements replace its children so repeated identical text still mutates the DOM.
//
// The region is emptied a moment after each announcement. A live region must stay in
// the accessibility tree to be announced at all (visually-hidden clips it from the
// screen but deliberately keeps it in the tree), which means its text is also
// reachable by browse-mode navigation — a screen reader arrowing from the buffer to
// the edit field would otherwise read stale announcement text as page content.
// Emptying is safe: the accessibility event fires on mutation and the screen reader
// copies the text into its own speech queue, so clearing the DOM afterwards cannot
// retract speech that has not been uttered yet. It is also silent, because the default
// `aria-relevant` is "additions text" — removals are not announced.

import type { AnnouncerView } from '../ports/announcer_view';

// How long an announcement stays in the DOM before the region is emptied. Long enough
// that the accessibility event has certainly been dispatched (that pipeline is
// milliseconds), short enough that stale text is rarely reachable. Tunable by feel
// from the manual NVDA pass.
const CLEAR_AFTER_MS = 1500;

export class AnnouncerDom implements AnnouncerView {
  private clearTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly region: HTMLElement) {}

  announce(text: string): void {
    this.region.replaceChildren();
    const line = document.createElement('div');
    line.textContent = text;
    this.region.append(line);

    // Restart the countdown on every announcement, so a burst of chunks is never
    // cleared out from under its own latest entry.
    clearTimeout(this.clearTimer);
    this.clearTimer = setTimeout(() => {
      this.region.replaceChildren();
    }, CLEAR_AFTER_MS);
  }
}
