// Role: adapter (DOM) — the single polite live region, created once in
// src/views/main_window.html and never recreated; drains go to an open dialog's
// `data-live-region` instead, because a modal makes the rest of the document inert.

import type { AnnouncerView } from '../ports/announcer_view';

const CLEAR_AFTER_MS = 1500;

// Measured with NVDA 2026.1.1 on WebView2: two announcements 75 ms or less apart are spoken
// as one utterance, 100 ms apart as two; this is about 2.5 times that threshold.
const DRAIN_SPACING_MS = 250;

// Measured with NVDA 2026.1.1: the first change into a region whose dialog just closed is lost;
// an empty node does not absorb that loss, a full stop does but is spoken, a zero-width space
// does and is silent.
const SILENT_MARKER = '\u200b';

// Holds the session's first drain: a live region changed while the page is still loading is
// not announced. A starting point, not a measurement.
const STARTUP_HOLD_MS = 1_000;

export class AnnouncerDom implements AnnouncerView {
  private readonly queue: string[] = [];
  private drainScheduled = false;
  private lastDrainAt = Number.NEGATIVE_INFINITY;
  /** Per region, so an announcement into one region cannot cancel another region's countdown. */
  private readonly clearTimers = new Map<
    HTMLElement,
    ReturnType<typeof setTimeout>
  >();
  private readonly openAt: number;

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

  /** Queues a wordless change so the next announcement is not the first after a dialog closed. */
  documentReturned(): void {
    this.announce(SILENT_MARKER);
  }

  /**
   * Resolves once the queue is empty and one DRAIN_SPACING_MS has passed since the last drain.
   *
   * Measured with NVDA 2026.1.1: resolving the moment the text was in the region left ten
   * milliseconds before a modal opened on top of it, and that text was never spoken.
   */
  drained(): Promise<void> {
    return new Promise<void>((resolve) => {
      const settle = (): void => {
        if (this.queue.length === 0 && !this.drainScheduled) {
          const owed = this.lastDrainAt + DRAIN_SPACING_MS - Date.now();
          if (owed <= 0) {
            resolve();
            return;
          }
          setTimeout(settle, owed);
          return;
        }
        setTimeout(settle, DRAIN_SPACING_MS);
      };
      settle();
    });
  }

  private scheduleDrain(): void {
    if (this.drainScheduled) {
      return;
    }
    this.drainScheduled = true;
    const sinceLastDrain = Date.now() - this.lastDrainAt;
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

  // Dialogs stack; the innermost is the last in document order only while every dialog is
  // written after the dialog that opens it.
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
