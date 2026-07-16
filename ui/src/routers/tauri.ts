// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.

import { invoke } from '@tauri-apps/api/core';

import type { BackendApi } from '../ports/backend_api';

export class TauriBackend implements BackendApi {
  echo(text: string): Promise<string> {
    return invoke<string>('echo', { text });
  }
}
