// Role: adapter (DOM) — the window's own title, its heading, and the status region.
//
// **Both titles are set explicitly, because one assignment does not do both.** A9 shipped
// believing that `document.title` in a Tauri window updates the native title as well; the
// user's NVDA said otherwise on 2026-08-25 — the report-title command still answered
// "Acter" while the document said "Acter - powershell". So the native title is a call
// through the shell port, and the document's is set here, and the two are set together in
// one method so they cannot drift.

import type { WindowView } from '../ports/window_view';

/** What the window is called with no far end behind it. */
const PRODUCT = 'Acter';

export class WindowChrome implements WindowView {
  constructor(
    private readonly heading: HTMLElement,
    private readonly statusRegion: HTMLElement,
    private readonly document: Document,
    /** Sets the operating system's own window title. */
    private readonly setNativeTitle: (title: string) => void,
  ) {}

  connectedTo(name: string | null): void {
    const title = name === null ? PRODUCT : `${PRODUCT} - ${name}`;
    // Three places, one value: the native title bar, the document, and the heading.
    this.setNativeTitle(title);
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
