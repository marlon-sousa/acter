// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.
// Outbound: typed invoke wrappers implementing BackendApi. Inbound: the JS
// Channel<SessionEvent> created for attachSession carries the session's event stream.

import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type { AboutFacts, AppShell } from '../ports/app_shell';
import type { BackendApi } from '../ports/backend_api';
import type { KeyAck, KeyPress, SessionEvent, SubmitAck } from '../protocol';

// Phase 1 has one session, connected automatically at startup (decision 9).
const SESSION_ID = 1;

export class TauriBackend implements BackendApi {
  async attachSession(onEvent: (event: SessionEvent) => void): Promise<void> {
    const channel = new Channel<SessionEvent>();
    channel.onmessage = onEvent;
    await invoke('attach_session', { sessionId: SESSION_ID, channel });
  }

  submitCommand(line: string): Promise<SubmitAck> {
    return invoke<SubmitAck>('submit_command', { sessionId: SESSION_ID, line });
  }

  sendKey(key: KeyPress): Promise<KeyAck> {
    return invoke<KeyAck>('send_key', { sessionId: SESSION_ID, key });
  }
}

export class TauriShell implements AppShell {
  about(): Promise<AboutFacts> {
    return invoke<AboutFacts>('about');
  }

  setTitle(title: string): Promise<void> {
    return getCurrentWindow().setTitle(title);
  }

  connection(): Promise<string | null> {
    return invoke<string | null>('connection');
  }

  platform(): Promise<string> {
    return invoke<string>('platform');
  }

  // Closing the window rather than exiting the process: Tauri drops the app's managed
  // state as the last window goes, which is what takes the session — and the shell it
  // spawned — with it.
  async exit(): Promise<void> {
    await getCurrentWindow().close();
  }
}
