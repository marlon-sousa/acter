// Role: test — the connect router's conversation loop.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ConnectStep } from '../../src/protocol';

const invoked: { cmd: string; args: Record<string, unknown> }[] = [];
let steps: { onmessage?: (step: ConnectStep) => void } | undefined;
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

const settle = () => new Promise((done) => setTimeout(done, 0));

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
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null);
    await settle();

    steps?.onmessage?.({
      step: 'Arrived',
      connected: {
        session: 3,
        label: 'Scripted: builtin',
        note: null,
        limit_explained: false,
        saved_as: null,
        line_owner: 'FarEnd',
      },
    });

    await expect(connecting).resolves.toEqual({
      session: 3,
      label: 'Scripted: builtin',
      note: null,
      limit_explained: false,
      saved_as: null,
      line_owner: 'FarEnd',
    });
  });

  it('rejects with the sentence a failed attempt ended on', async () => {
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null);
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
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null, {
      onProgress: (sentence: string) => said.push(sentence),
    });
    await settle();

    steps?.onmessage?.({ step: 'Progress', said: 'Connecting to acter-ssh.' });
    steps?.onmessage?.({
      step: 'Arrived',
      connected: {
        session: 1,
        label: 'x',
        note: null,
        limit_explained: false,
        saved_as: null,
        line_owner: 'FarEnd',
      },
    });
    await connecting;

    expect(said).toEqual(['Connecting to acter-ssh.']);
  });

  it('answers against the attempt that asked', async () => {
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null, {
      onQuestion: () => Promise.resolve({ answer: 'Trust' as const }),
    });
    await settle();

    steps?.onmessage?.(asked(42));
    await settle();

    expect(invoked).toContainEqual({
      cmd: 'answer_connect',
      args: { attempt: 42, answer: { answer: 'Trust' } },
    });

    steps?.onmessage?.({
      step: 'Arrived',
      connected: {
        session: 1,
        label: 'x',
        note: null,
        limit_explained: false,
        saved_as: null,
        line_owner: 'FarEnd',
      },
    });
    await connecting;
  });

  it('gives up on a question when nobody can be asked', async () => {
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null);
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

  it('tells the backend to forget an attempt that ended', async () => {
    const connecting = new TauriConnect().use(PROFILE, 'Yes', null);
    await settle();

    steps?.onmessage?.({
      step: 'Arrived',
      connected: {
        session: 1,
        label: 'x',
        note: null,
        limit_explained: false,
        saved_as: null,
        line_owner: 'FarEnd',
      },
    });
    await connecting;
    await settle();

    expect(invoked.map((each) => each.cmd)).toContain('attempt_ended');
  });
});
