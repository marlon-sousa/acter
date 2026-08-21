// @vitest-environment jsdom
// Role: test — the debug recorder records the protocol in arrival order, and does not
// exist at all unless the backend said this is a debug build.

import { beforeEach, describe, expect, it } from 'vitest';

import { installDebugRecorder } from '../../src/adapters/debug_recorder';
import type { DebugEntry } from '../../src/adapters/debug_recorder';
import type { BackendApi } from '../../src/ports/backend_api';
import type { KeyAck, KeyPress, SessionEvent, SubmitAck } from '../../src/protocol';

class FakeBackend implements BackendApi {
  private onEvent: ((event: SessionEvent) => void) | undefined;
  keyAck: KeyAck = 'Applied';

  attachSession(onEvent: (event: SessionEvent) => void): Promise<void> {
    this.onEvent = onEvent;
    return Promise.resolve();
  }
  submitCommand(): Promise<SubmitAck> {
    return Promise.resolve({ command_id: 7 });
  }
  sendKey(): Promise<KeyAck> {
    return Promise.resolve(this.keyAck);
  }
  emit(event: SessionEvent): void {
    this.onEvent?.(event);
  }
}

interface DebugWindow {
  __ACTER_DEBUG__?: boolean;
  __acterDebug?: { entries(): DebugEntry[]; clear(): void };
}

function debugWindow(): DebugWindow {
  return window as unknown as DebugWindow;
}

beforeEach(() => {
  delete debugWindow().__ACTER_DEBUG__;
  delete debugWindow().__acterDebug;
});

describe('installDebugRecorder in a release build', () => {
  // The flag comes from a `#[cfg(debug_assertions)]` plugin, so its absence *is* the
  // release build. Nothing may be installed and nothing may be wrapped.
  it('hands the backend back untouched and installs nothing', () => {
    const backend = new FakeBackend();

    const wrapped = installDebugRecorder(backend);

    expect(wrapped).toBe(backend);
    expect(debugWindow().__acterDebug).toBeUndefined();
  });
});

describe('installDebugRecorder in a debug build', () => {
  beforeEach(() => {
    debugWindow().__ACTER_DEBUG__ = true;
  });

  it('records events in arrival order, which is the thing it exists for', async () => {
    const backend = new FakeBackend();
    const wrapped = installDebugRecorder(backend);
    await wrapped.attachSession(() => {});

    backend.emit({ type: 'Output', command_id: 1, text: 'hello' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 30 },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    const events = debugWindow()
      .__acterDebug!.entries()
      .filter((entry) => entry.kind === 'event')
      .map((entry) => entry.what);
    expect(events).toEqual(['Output', 'Announce', 'CommandFinished']);
  });

  // The record is the arrival order, not the handling order: an entry is written before
  // the controller's handler runs, so a handler that reorders its own work cannot make
  // the tape agree with it.
  it('records an event before passing it on', async () => {
    const backend = new FakeBackend();
    const wrapped = installDebugRecorder(backend);
    let seenWhileHandling: string[] = [];
    await wrapped.attachSession(() => {
      seenWhileHandling = debugWindow()
        .__acterDebug!.entries()
        .filter((entry) => entry.kind === 'event')
        .map((entry) => entry.what);
    });

    backend.emit({ type: 'CommandStarted', command_id: 1 });

    expect(seenWhileHandling).toEqual(['CommandStarted']);
  });

  it('records each call and the ack that answered it', async () => {
    const wrapped = installDebugRecorder(new FakeBackend());
    const key: KeyPress = { key: { Char: 'c' }, ctrl: true, shift: false, alt: false };

    await wrapped.submitCommand('small');
    await wrapped.sendKey(key);

    const traffic = debugWindow()
      .__acterDebug!.entries()
      .filter((entry) => entry.kind !== 'event')
      .map((entry) => `${entry.kind} ${entry.what}`);
    expect(traffic).toEqual([
      'call submitCommand',
      'ack submitCommand',
      'call sendKey',
      'ack sendKey',
    ]);
  });

  it('numbers entries monotonically so a dropped prefix stays obvious', async () => {
    const backend = new FakeBackend();
    const wrapped = installDebugRecorder(backend);
    await wrapped.attachSession(() => {});
    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    const seqs = debugWindow()
      .__acterDebug!.entries()
      .map((entry) => entry.seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    expect(new Set(seqs).size).toBe(seqs.length);
  });

  it('hands out a copy, so a reader cannot mutate the record', async () => {
    const backend = new FakeBackend();
    const wrapped = installDebugRecorder(backend);
    await wrapped.attachSession(() => {});
    backend.emit({ type: 'CommandStarted', command_id: 1 });

    debugWindow().__acterDebug!.entries().length = 0;

    expect(debugWindow().__acterDebug!.entries()).toHaveLength(2);
  });

  it('clears on request', async () => {
    const wrapped = installDebugRecorder(new FakeBackend());
    await wrapped.attachSession(() => {});

    debugWindow().__acterDebug!.clear();

    expect(debugWindow().__acterDebug!.entries()).toEqual([]);
  });
});
