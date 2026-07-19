// Role: test — locks the generated protocol contract the inbound channel router
// (A3) is built against: the SessionEvent union stays exhaustively handleable, and
// adding a Rust variant without handling it here fails `tsc`.

import { describe, expect, it } from 'vitest';

import type { SessionEvent, SubmitAck } from '../src/protocol';

function assertNever(x: never): never {
  throw new Error(`unhandled SessionEvent variant: ${JSON.stringify(x)}`);
}

// A total function over every SessionEvent variant. If the generated union grows a
// variant that this switch does not handle, the `assertNever(event)` call stops
// type-checking — the compile-time guard A3's router relies on.
function describeEvent(event: SessionEvent): string {
  switch (event.type) {
    case 'CommandStarted':
      return `started ${event.command_id}`;
    case 'Output':
      return `output ${event.command_id} (${event.read_mode}): ${event.text}`;
    case 'CommandFinished':
      return `finished ${event.command_id} exit ${event.exit_code} (${event.read_mode})`;
    case 'CommandStillRunning':
      return `still running ${event.command_id}`;
    case 'AltScreenEntered':
      return 'alt-screen entered';
    case 'AltScreenLeft':
      return 'alt-screen left';
    case 'TitleChanged':
      return `title ${event.title}`;
    case 'ConnectionChanged':
      return `connection ${event.state}`;
    default:
      return assertNever(event);
  }
}

describe('protocol bindings', () => {
  it('discriminates every SessionEvent variant on `type`', () => {
    const output: SessionEvent = {
      type: 'Output',
      command_id: 1,
      text: 'hello',
      read_mode: 'Auto',
    };
    expect(describeEvent(output)).toBe('output 1 (Auto): hello');

    const finished: SessionEvent = {
      type: 'CommandFinished',
      command_id: 1,
      exit_code: 0,
      read_mode: 'Quiet',
    };
    expect(describeEvent(finished)).toBe('finished 1 exit 0 (Quiet)');

    const altScreen: SessionEvent = { type: 'AltScreenEntered' };
    expect(describeEvent(altScreen)).toBe('alt-screen entered');
  });

  it('types SubmitAck as the correlation id return payload', () => {
    const ack: SubmitAck = { command_id: 42 };
    expect(ack.command_id).toBe(42);
  });
});
