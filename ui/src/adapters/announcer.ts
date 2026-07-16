// Role: adapter (DOM) — the single polite live region. The region element is created
// once in src/views/main_window.html and never recreated (live-region lifecycle rule);
// announcements replace its children so repeated identical text still mutates the DOM.

import type { AnnouncerView } from '../ports/announcer_view';

export class AnnouncerDom implements AnnouncerView {
  constructor(private readonly region: HTMLElement) {}

  announce(text: string): void {
    this.region.replaceChildren();
    const line = document.createElement('div');
    line.textContent = text;
    this.region.append(line);
  }
}
