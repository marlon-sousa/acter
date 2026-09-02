// Role: test — locks the generated protocol contract the inbound channel router
// (A3) is built against: the SessionEvent union stays exhaustively handleable, and
// adding a Rust variant without handling it here fails `tsc`.

import { describe, expect, it } from 'vitest';

import type { Announcement, SessionEvent, SubmitAck } from '../src/protocol';

function assertNever(x: never): never {
  throw new Error(`unhandled SessionEvent variant: ${JSON.stringify(x)}`);
}

// The same guard one level down: Announcement is a nested union, so a new kind added
// in Rust has to be handled here too or `tsc` fails.
function describeAnnouncement(announcement: Announcement): string {
  switch (announcement.kind) {
    case 'ReadAloud':
      return `read ${announcement.text}`;
    case 'TooBig':
      return `too big ${announcement.lines}`;
    case 'StillRunning':
      return 'still running';
    case 'OutputContinues':
      return 'output continues';
    case 'Failed':
      return `failed ${announcement.exit_code}`;
    default:
      return assertNever(announcement);
  }
}

// A total function over every SessionEvent variant. If the generated union grows a
// variant that this switch does not handle, the `assertNever(event)` call stops
// type-checking — the compile-time guard A3's router relies on.
function describeEvent(event: SessionEvent): string {
  switch (event.type) {
    case 'CommandStarted':
      return `started ${event.command_id}`;
    case 'Output':
      return `output ${event.command_id}: ${event.text}`;
    case 'CommandFinished':
      return `finished ${event.command_id}`;
    case 'CommandInterrupted':
      return `interrupted ${event.command_id}`;
    case 'IntegrationUnavailable':
      return 'integration unavailable';
    case 'AltScreenEntered':
      return 'alt-screen entered';
    case 'AltScreenLeft':
      return 'alt-screen left';
    case 'TitleChanged':
      return `title ${event.title}`;
    case 'ConnectionChanged':
      return `connection ${event.state}`;
    case 'Announce':
      return `announce ${event.command_id}: ${describeAnnouncement(event.announcement)}`;
    case 'PromptDrawn':
      return `prompt ${event.text}`;
    case 'FarEndLine':
      return `far end line ${event.text ?? '(unchanged)'} at ${event.caret}`;
    default:
      return assertNever(event);
  }
}

describe('protocol bindings', () => {
  it('discriminates every SessionEvent variant on `type`', () => {
    const output: SessionEvent = {
      type: 'Output',
      command_id: 1,
      line: 4,
      revision: 'Appended',
      text: 'hello',
    };
    expect(describeEvent(output)).toBe('output 1: hello');

    const finished: SessionEvent = { type: 'CommandFinished', command_id: 1 };
    expect(describeEvent(finished)).toBe('finished 1');

    const altScreen: SessionEvent = { type: 'AltScreenEntered' };
    expect(describeEvent(altScreen)).toBe('alt-screen entered');

    const announce: SessionEvent = {
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 120 },
    };
    expect(describeEvent(announce)).toBe('announce 1: too big 120');
  });

  it('types SubmitAck as the correlation id return payload, or a refusal', () => {
    const ack: SubmitAck = { status: 'Accepted', command_id: 42 };
    expect(ack.status === 'Accepted' && ack.command_id).toBe(42);

    // The other answer, which is what a window with no session behind it gives (B7). It
    // carries no id at all, so a frontend cannot read a refusal as "command zero".
    const refused: SubmitAck = { status: 'NotConnected' };
    expect(refused).not.toHaveProperty('command_id');
  });
});
