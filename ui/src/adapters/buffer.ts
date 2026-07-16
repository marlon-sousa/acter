// Role: adapter (DOM) — the results buffer region: one h2 per command, response below.

import type { BufferView } from '../ports/buffer_view';

export class BufferDom implements BufferView {
  constructor(private readonly region: HTMLElement) {}

  appendBlock(command: string, response: string): void {
    const heading = document.createElement('h2');
    heading.textContent = command;
    // Programmatically focusable (a heading is never in the tab order) so
    // focus() can land here without adding it to sequential navigation.
    heading.tabIndex = -1;
    const body = document.createElement('div');
    body.className = 'response';
    body.textContent = response;
    this.region.append(heading, body);
  }

  focus(): void {
    const headings = this.region.querySelectorAll('h2');
    const mostRecent = headings[headings.length - 1];
    (mostRecent ?? this.region).focus();
  }

  containsFocus(): boolean {
    return this.region.contains(document.activeElement);
  }
}
