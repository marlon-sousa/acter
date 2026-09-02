// Role: adapter (debug) — a decorator over `BackendApi` that records the frontend's
// whole view of the protocol into a bounded ring, readable from the automation harness.
//
// **Why this exists.** The frontend's bugs are mostly *ordering* bugs, and ordering is
// the one thing neither the DOM nor the live region shows: by the time a wrong order has
// had its effect, the evidence is gone. A6's beep defect is the case in point — the
// too-big verdict arrived one event after the finish that was supposed to act on it, and
// nothing visible said so. What was missing was never audio; it was the sequence.
//
// So this records what arrived and what was asked, in order, with a sequence number and a
// timestamp, and hands it to whoever is driving the app. It reads rather than acts: it
// changes no behavior, answers no question for the app itself, and the controller never
// learns it exists.
//
// **Debug builds only.** The backend injects `window.__ACTER_DEBUG__` from a
// `#[cfg(debug_assertions)]` plugin, exactly as it registers the embedded WebDriver
// server (spec T2), so a release binary carries neither. With the flag absent this module
// hands back the backend it was given, unwrapped, and nothing is installed on `window`.

import type { BackendApi } from '../ports/backend_api';
import type {
  KeyAck,
  KeyPress,
  LineOwner,
  SessionEvent,
  SessionId,
  SubmitAck,
} from '../protocol';

/** How many entries the ring holds before the oldest is dropped. */
const CAPACITY = 1000;

/** One thing that crossed the port, in the direction it crossed. */
export interface DebugEntry {
  /** Monotonic across the session, so a dropped prefix is still obvious. */
  seq: number;
  /** Milliseconds since the page loaded, monotonic and immune to clock changes. */
  at: number;
  /** `event` inbound from the session; `call` outbound; `ack` the answer to a call. */
  kind: 'event' | 'call' | 'ack';
  /** The event's `type`, or the method's name. */
  what: string;
  /** The payload, structured-clone-safe for the WebDriver bridge. */
  detail: unknown;
}

interface DebugWindow {
  __ACTER_DEBUG__?: boolean;
  __acterDebug?: {
    entries(): DebugEntry[];
    clear(): void;
  };
}

class Ring {
  private readonly entries: DebugEntry[] = [];
  private seq = 0;

  push(kind: DebugEntry['kind'], what: string, detail: unknown): void {
    this.entries.push({
      seq: this.seq++,
      at: Math.round(performance.now()),
      kind,
      what,
      detail,
    });
    if (this.entries.length > CAPACITY) {
      this.entries.shift();
    }
  }

  read(): DebugEntry[] {
    // A copy: the harness reads across a bridge that serializes, and a caller must never
    // be able to mutate the record it is inspecting.
    return this.entries.map((entry) => ({ ...entry }));
  }

  clear(): void {
    this.entries.length = 0;
  }
}

class RecordingBackend implements BackendApi {
  constructor(
    private readonly inner: BackendApi,
    private readonly ring: Ring,
  ) {}

  // The session id is recorded with every call since B7: a window that connected twice
  // has two sessions in its record, and which one a call belonged to is exactly the sort
  // of ordering question this ring exists to answer.
  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void> {
    this.ring.push('call', 'attachSession', { session });
    // Recorded before the controller sees it, so the record is the arrival order rather
    // than the handling order — which is the distinction an ordering bug turns on.
    return this.inner.attachSession(session, (event) => {
      this.ring.push('event', event.type, event);
      onEvent(event);
    });
  }

  async submitCommand(session: SessionId, line: string): Promise<SubmitAck> {
    this.ring.push('call', 'submitCommand', { session, line });
    const ack = await this.inner.submitCommand(session, line);
    this.ring.push('ack', 'submitCommand', ack);
    return ack;
  }

  async sendKey(session: SessionId, key: KeyPress): Promise<KeyAck> {
    this.ring.push('call', 'sendKey', { session, key });
    const ack = await this.inner.sendKey(session, key);
    this.ring.push('ack', 'sendKey', ack);
    return ack;
  }

  // Recorded like every other call, and for the reason the ring exists: which line owner
  // was in force when a keystroke went out is exactly the ordering question a far-end
  // session raises, and it is unanswerable from the keystrokes alone.
  setLineOwner(session: SessionId, owner: LineOwner): Promise<void> {
    this.ring.push('call', 'setLineOwner', { session, owner });
    return this.inner.setLineOwner(session, owner);
  }

  paste(session: SessionId, text: string): Promise<void> {
    this.ring.push('call', 'paste', { session, text });
    return this.inner.paste(session, text);
  }
}

/**
 * Wrap `backend` in a recorder when this is a debug build, and install the reader at
 * `window.__acterDebug`. In a release build the backend is returned untouched and
 * nothing is installed.
 */
export function installDebugRecorder(backend: BackendApi): BackendApi {
  const target = window as unknown as DebugWindow;
  if (target.__ACTER_DEBUG__ !== true) {
    return backend;
  }
  const ring = new Ring();
  target.__acterDebug = {
    entries: () => ring.read(),
    clear: () => {
      ring.clear();
    },
  };
  return new RecordingBackend(backend, ring);
}
