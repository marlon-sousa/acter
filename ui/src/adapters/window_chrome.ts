// Role: adapter (DOM) — the window's own title, its heading, and the status region.
//
// The document title is set here rather than through Tauri's window API on purpose: setting
// `document.title` in a Tauri window updates the native title too, so one assignment keeps
// both in step and the frontend needs no IPC to say what it is (spec A9, decision 1).

import type { WindowView } from '../ports/window_view';

/** What the window is called with no far end behind it. */
const PRODUCT = 'Acter';

export class WindowChrome implements WindowView {
  constructor(
    private readonly heading: HTMLElement,
    private readonly statusRegion: HTMLElement,
    private readonly document: Document,
  ) {}

  connectedTo(name: string | null): void {
    const title = name === null ? PRODUCT : `${PRODUCT} - ${name}`;
    // Both, from one value, in one place.
    this.document.title = title;
    this.heading.textContent = title;
  }

  status(text: string): void {
    // Written only when it changes. A live region that is reassigned the same text can
    // still fire an accessibility event, and a status that repeats itself for no reason is
    // a status a listener learns to ignore.
    if (this.statusRegion.textContent !== text) {
      this.statusRegion.textContent = text;
    }
  }
}
