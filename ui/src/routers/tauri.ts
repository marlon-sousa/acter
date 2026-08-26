// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.
// Outbound: typed invoke wrappers implementing BackendApi and ConnectApi. Inbound: the JS
// Channel<SessionEvent> created for attachSession carries the session's event stream.

import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type { AboutFacts, AppShell } from '../ports/app_shell';
import type { BackendApi } from '../ports/backend_api';
import type { ConnectApi } from '../ports/connect_api';
import type {
  Connectable,
  Connected,
  KeyAck,
  KeyPress,
  ProfileId,
  SessionEvent,
  SessionId,
  SubmitAck,
} from '../protocol';

export class TauriBackend implements BackendApi {
  async attachSession(
    session: SessionId,
    onEvent: (event: SessionEvent) => void,
  ): Promise<void> {
    const channel = new Channel<SessionEvent>();
    channel.onmessage = onEvent;
    await invoke('attach_session', { sessionId: session, channel });
  }

  submitCommand(session: SessionId, line: string): Promise<SubmitAck> {
    return invoke<SubmitAck>('submit_command', { sessionId: session, line });
  }

  sendKey(session: SessionId, key: KeyPress): Promise<KeyAck> {
    return invoke<KeyAck>('send_key', { sessionId: session, key });
  }
}

// The three named actions of B7, each one invoke. `use` rejects with the sentence the
// backend wrote when a far end cannot be started, which is what Tauri does with an `Err`
// String — so the caller says it and the session that was running is untouched.
export class TauriConnect implements ConnectApi {
  connectable(): Promise<Connectable[]> {
    return invoke<Connectable[]>('connectable');
  }

  use(id: ProfileId): Promise<Connected> {
    return invoke<Connected>('use_profile', { profile: id });
  }

  connected(): Promise<Connected | null> {
    return invoke<Connected | null>('connected');
  }
}

export class TauriShell implements AppShell {
  about(): Promise<AboutFacts> {
    return invoke<AboutFacts>('about');
  }

  setTitle(title: string): Promise<void> {
    return getCurrentWindow().setTitle(title);
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
