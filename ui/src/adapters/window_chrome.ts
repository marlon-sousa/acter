// Role: adapter (DOM) — what the window is: its titles, its status region, and which of
// its two faces it is showing.
// In a Tauri window `document.title` does not set the native title: NVDA's report-title command
// still answered the old one.

import type { WindowView } from '../ports/window_view';

const PRODUCT = 'Acter';

// Measured with NVDA 2026.1.1: focus placed while the page is still loading leaves the browse
// cursor behind, so the first Enter acts on the cursor. The value is a starting point, not a
// measurement.
const STARTUP_HOLD_MS = 400;

export interface WindowElements {
  heading: HTMLElement;
  statusRegion: HTMLElement;
  notConnectedWindow: HTMLElement;
  connectButton: HTMLElement;
  terminalWindow: HTMLElement;
  form: HTMLElement;
  editField: { focus(): void };
  farEndField: { focus(): void };
  ended: HTMLElement;
  reconnectButton: HTMLElement;
  document: Document;
  setNativeTitle: (title: string) => void;
}

export class WindowChrome implements WindowView {
  private opened = false;
  private hasConnected = false;

  constructor(
    private readonly elements: WindowElements,
    private readonly startupHold: number = STARTUP_HOLD_MS,
  ) {}

  focus(): void {
    this.landing().focus();
  }

  private landing(): { focus(): void } {
    if (this.elements.terminalWindow.hidden) {
      return this.elements.connectButton;
    }
    if (!this.elements.ended.hidden) {
      return this.elements.reconnectButton;
    }
    return this.elements.form.hidden
      ? this.elements.farEndField
      : this.elements.editField;
  }

  connectedTo(name: string | null): void {
    const title = name === null ? PRODUCT : `${PRODUCT} - ${name}`;
    this.elements.setNativeTitle(title);
    this.elements.document.title = title;
    this.elements.heading.textContent = title;
  }

  status(text: string): void {
    // A live region reassigned the same text can still fire an accessibility event.
    if (this.elements.statusRegion.textContent !== text) {
      this.elements.statusRegion.textContent = text;
    }
  }

  // Focus moves only if it was in what just went away or nowhere, so a reader keeps their place.
  showTerminal(live: boolean): void {
    const { notConnectedWindow, terminalWindow, form, ended, document } = this.elements;
    const active = document.activeElement;
    const stranded =
      active === null ||
      active === document.body ||
      notConnectedWindow.contains(active) ||
      form.contains(active) ||
      ended.contains(active);

    this.hasConnected = this.hasConnected || live;

    notConnectedWindow.hidden = this.hasConnected;
    terminalWindow.hidden = !this.hasConnected;
    form.hidden = !live;
    ended.hidden = live;

    if (!stranded) {
      return;
    }
    if (this.opened || this.startupHold === 0) {
      this.opened = true;
      this.focus();
      return;
    }
    this.opened = true;
    setTimeout(() => this.focus(), this.startupHold);
  }

  showLocalLine(showing: boolean): void {
    this.elements.form.hidden = !showing;
  }
}
