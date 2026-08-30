// Role: adapter — the Tauri IPC router; the only module importing @tauri-apps/api.
// Outbound: typed invoke wrappers implementing BackendApi and ConnectApi. Inbound: the JS
// Channel<SessionEvent> created for attachSession carries the session's event stream.

import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type { AboutFacts, AppShell } from '../ports/app_shell';
import type { BackendApi } from '../ports/backend_api';
import type { ConnectApi, ConnectListener } from '../ports/connect_api';
import type {
  AttemptId,
  Connectable,
  ConnectAnswer,
  ConnectStep,
  Connected,
  KeyAck,
  KeyPress,
  ProfileId,
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
}

// B7's three named actions, with `use` rebuilt on B9's conversation.
//
// **`use` still answers with a session or rejects with a sentence, and that is deliberate.**
// Connecting is now a stream of steps — progress, questions, and finally an outcome — but
// only SSH ever asks anything, and hiding the stream behind the promise the rest of this
// frontend already understood is what let B9 land without rewriting the controller, the
// dialog, or their tests. The `onQuestion` hook is where the dialogs attach; a caller that
// passes none is saying it cannot answer, and a far end that asks is told so.
export class TauriConnect implements ConnectApi {
  connectable(): Promise<Connectable[]> {
    return invoke<Connectable[]>('connectable');
  }

  use(
    id: ProfileId,
    setUp: SetUp,
    listener: ConnectListener = {},
  ): Promise<Connected> {
    return new Promise<Connected>((resolve, reject) => {
      const steps = new Channel<ConnectStep>();
      // Held so the terminal step can tell the backend to forget this attempt, and so a
      // question can be answered against the attempt that asked it rather than whichever
      // is in flight.
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
              // Nothing here can ask a person, so the honest answer is that nobody
              // answered — which the backend reads as a refusal (spec B9, decision 3).
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
            // Rejecting with the sentence the backend wrote keeps this the same shape a
            // caller has handled since B7: it says it, and the session that was running is
            // untouched.
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
        steps,
      }).then((started) => {
        // The id is needed before any answer can be sent, and a question can in principle
        // arrive before this resolves — so the step handler sets it too, and whichever
        // arrives first wins. They are the same value.
        attempt ??= started;
      });
    });
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
