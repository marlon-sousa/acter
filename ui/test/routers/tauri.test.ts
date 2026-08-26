// Role: test — the connect router's conversation loop.
//
// **Routers are normally exempt from testing** as pure glue with no branches
// (ARCHITECTURE). `TauriConnect.use` stopped qualifying with B9: it reads a stream of
// steps, decides which of them ends the attempt, routes a question to whoever can answer
// it, and answers on behalf of a caller who cannot. Those are branches, and the one that
// matters most — answering against the attempt that asked — is exactly the kind of mistake
// that would deliver a password to the wrong question.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ConnectStep } from '../../src/protocol';

/** Every invoke the router made, in order. */
const invoked: { cmd: string; args: Record<string, unknown> }[] = [];
/** The channel the router handed to `use_profile`, so a test can push steps down it. */
let steps: { onmessage?: (step: ConnectStep) => void } | undefined;
/** What `use_profile` answers with. */
let attemptId = 7;

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage?: (step: ConnectStep) => void;
  },
  invoke: (cmd: string, args: Record<string, unknown>) => {
    invoked.push({ cmd, args });
    if (cmd === 'use_profile') {
      steps = args.steps as { onmessage?: (step: ConnectStep) => void };
      return Promise.resolve(attemptId);
    }
    return Promise.resolve(undefined);
  },
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTitle: () => Promise.resolve() }),
}));

const { TauriConnect } = await import('../../src/routers/tauri');

const PROFILE = { profile: 'Scripted', name: 'builtin' } as const;

/** Lets the promise the router is awaiting settle before the test looks. */
const settle = () => new Promise((done) => setTimeout(done, 0));

/** The step that says a host key needs a decision. */
const asked = (attempt: number): ConnectStep => ({
  step: 'Asked',
  attempt,
  question: {
    question: 'HostKey',
    host: 'acter-ssh',
    port: 2222,
    fingerprint: 'SHA256:offered',
    recorded: null,
    aside: null,
  },
});

describe('TauriConnect.use', () => {
  beforeEach(() => {
    invoked.length = 0;
    steps = undefined;
    attemptId = 7;
  });

  it('resolves with the session the conversation arrived at', async () => {
    const connecting = new TauriConnect().use(PROFILE);
    await settle();

    steps?.onmessage?.({
      step: 'Arrived',
      connected: { session: 3, label: 'Scripted: builtin', note: null },
    });

    await expect(connecting).resolves.toEqual({
      session: 3,
      label: 'Scripted: builtin',
      note: null,
    });
  });

  // The sentence is the backend's, and rejecting with it keeps this the shape every caller
  // has handled since B7 — they say it, and the session that was running is untouched.
  it('rejects with the sentence a failed attempt ended on', async () => {
    const connecting = new TauriConnect().use(PROFILE);
    await settle();

    steps?.onmessage?.({
      step: 'Failed',
      why: 'Acter could not reach acter-ssh on port 2222.',
    });

    await expect(connecting).rejects.toBe(
      'Acter could not reach acter-ssh on port 2222.',
    );
  });

  it('passes progress to a caller that wants to hear it', async () => {
    const said: string[] = [];
    const connecting = new TauriConnect().use(PROFILE, {
      onProgress: (sentence) => said.push(sentence),
    });
    await settle();

    steps?.onmessage?.({ step: 'Progress', said: 'Connecting to acter-ssh.' });
    steps?.onmessage?.({ step: 'Arrived', connected: { session: 1, label: 'x', note: null } });
    await connecting;

    expect(said).toEqual(['Connecting to acter-ssh.']);
  });

  // **The one that matters.** An answer carries the attempt the question came from, never
  // whichever attempt happens to be in flight — a password delivered to the wrong question
  // is the worst version of being helpful.
  it('answers against the attempt that asked', async () => {
    const connecting = new TauriConnect().use(PROFILE, {
      onQuestion: () => Promise.resolve({ answer: 'Trust' as const }),
    });
    await settle();

    steps?.onmessage?.(asked(42));
    await settle();

    expect(invoked).toContainEqual({
      cmd: 'answer_connect',
      args: { attempt: 42, answer: { answer: 'Trust' } },
    });

    steps?.onmessage?.({ step: 'Arrived', connected: { session: 1, label: 'x', note: null } });
    await connecting;
  });

  // **A caller who cannot ask anybody is not a caller who trusts everybody.** Nothing was
  // answered, and the backend reads that as a refusal (spec B9, decision 3).
  it('gives up on a question when nobody can be asked', async () => {
    const connecting = new TauriConnect().use(PROFILE);
    await settle();

    steps?.onmessage?.(asked(42));
    await settle();

    expect(invoked).toContainEqual({
      cmd: 'answer_connect',
      args: { attempt: 42, answer: { answer: 'GiveUp' } },
    });

    steps?.onmessage?.({
      step: 'Failed',
      why: 'Acter did not connect, because the host key was not accepted.',
    });
    await expect(connecting).rejects.toBeTruthy();
  });

  // The backend keeps an attempt alive until the window says it is done with it, so a
  // conversation that ended has to say so or the map grows for the life of the process.
  it('tells the backend to forget an attempt that ended', async () => {
    const connecting = new TauriConnect().use(PROFILE);
    await settle();

    steps?.onmessage?.({ step: 'Arrived', connected: { session: 1, label: 'x', note: null } });
    await connecting;
    await settle();

    expect(invoked.map((each) => each.cmd)).toContain('attempt_ended');
  });
});
