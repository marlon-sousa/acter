// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type { AboutFacts, AppShell } from '../ports/app_shell';
import type { BackendApi } from '../ports/backend_api';
import type { ConnectApi, ConnectListener } from '../ports/connect_api';
import type { SystemMenuEvents } from '../ports/system_menu';
import type {
  AttemptId,
  Connectable,
  ConnectAnswer,
  ConnectStep,
  Connected,
  KeyAck,
  KeyPress,
  LaunchRequest,
  LineOwner,
  MenuAction,
  ProfileId,
  SavedConnections,
  SessionEvent,
  SessionId,
  SetUp,
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

  setLineOwner(session: SessionId, owner: LineOwner): Promise<void> {
    return invoke('set_line_owner', { sessionId: session, owner });
  }

  paste(session: SessionId, text: string): Promise<void> {
    return invoke('paste', { sessionId: session, text });
  }
}

export class TauriConnect implements ConnectApi {
  connectable(): Promise<Connectable[]> {
    return invoke<Connectable[]>('connectable');
  }

  use(
    id: ProfileId,
    setUp: SetUp,
    origin: string | null,
    listener: ConnectListener = {},
  ): Promise<Connected> {
    return new Promise<Connected>((resolve, reject) => {
      const steps = new Channel<ConnectStep>();
      let attempt: AttemptId | null = null;

      const answer = (given: ConnectAnswer): void => {
        if (attempt !== null) {
          void invoke('answer_connect', { attempt, answer: given });
        }
      };

      steps.onmessage = (step) => {
        switch (step.step) {
          case 'Progress':
            listener.onProgress?.(step.said);
            break;
          case 'Asked':
            attempt = step.attempt;
            if (listener.onQuestion === undefined) {
              answer({ answer: 'GiveUp' });
              break;
            }
            void listener.onQuestion(step.question).then(answer);
            break;
          case 'Arrived':
            done();
            resolve(step.connected);
            break;
          case 'Failed':
            done();
            reject(step.why);
            break;
        }
      };

      const done = (): void => {
        if (attempt !== null) {
          void invoke('attempt_ended', { attempt });
        }
      };

      void invoke<AttemptId>('use_profile', {
        profile: id,
        setUp,
        origin,
        steps,
      }).then((started) => {
        // A question can arrive before this resolves, so the step handler sets it too.
        attempt ??= started;
      });
    });
  }

  connected(): Promise<Connected | null> {
    return invoke<Connected | null>('connected');
  }

  saved(): Promise<SavedConnections> {
    return invoke<SavedConnections>('saved');
  }

  saveConnection(name: string): Promise<string> {
    return invoke<string>('save_connection', { name });
  }

  renameConnection(from: string, to: string): Promise<string> {
    return invoke<string>('rename_connection', { from, to });
  }

  forgetConnection(name: string): Promise<string> {
    return invoke<string>('forget_connection', { name });
  }

  offerToSave(): Promise<boolean> {
    return invoke<boolean>('offer_to_save');
  }

  stopOfferingToSave(): Promise<void> {
    return invoke('stop_offering_to_save');
  }

  requestedAtLaunch(): Promise<LaunchRequest | null> {
    return invoke<LaunchRequest | null>('requested_at_launch');
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

  // Tauri drops the app's managed state, and with it the session and its shell, when the
  // last window closes.
  async exit(): Promise<void> {
    await getCurrentWindow().close();
  }
}

// The event name must match `MENU_EVENT` in crates/acter-app/src/adapters/system_menu.rs.
export class TauriSystemMenu implements SystemMenuEvents {
  onChosen(chosen: (action: MenuAction) => void): void {
    void listen<MenuAction>('acter://menu', (event) => chosen(event.payload));
  }
}
