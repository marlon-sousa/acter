// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.
// Outbound: typed invoke wrappers implementing BackendApi. Inbound: the JS
// Channel<SessionEvent> created for attachSession carries the session's event stream.

import { Channel, invoke } from '@tauri-apps/api/core';

import type { BackendApi } from '../ports/backend_api';
import type { SessionEvent, SubmitAck } from '../protocol';

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
}
