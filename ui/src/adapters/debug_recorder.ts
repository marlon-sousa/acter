// Role: adapter (debug) — a decorator over `BackendApi` that records the frontend's
// whole view of the protocol into a bounded ring, readable from the automation harness.
// `window.__ACTER_DEBUG__` is injected only by a `#[cfg(debug_assertions)]` plugin; without it
// the backend is returned unwrapped and nothing is installed.

import type { BackendApi } from '../ports/backend_api';
import type {
  KeyAck,
  KeyPress,
  LineOwner,
  SessionEvent,
  SessionId,
  SubmitAck,
} from '../protocol';

const CAPACITY = 1000;

export interface DebugEntry {
  seq: number;
  at: number;
  /** `event` inbound from the session; `call` outbound; `ack` the answer to a call. */
  kind: 'event' | 'call' | 'ack';
  what: string;
  /** Must stay structured-clone-safe for the WebDriver bridge. */
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

  attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void> {
    this.ring.push('call', 'attachSession', { session });
    // Recorded before the controller sees it, so the record is arrival order, not handling order.
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

  setLineOwner(session: SessionId, owner: LineOwner): Promise<void> {
    this.ring.push('call', 'setLineOwner', { session, owner });
    return this.inner.setLineOwner(session, owner);
  }

  paste(session: SessionId, text: string): Promise<void> {
    this.ring.push('call', 'paste', { session, text });
    return this.inner.paste(session, text);
  }
}

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
