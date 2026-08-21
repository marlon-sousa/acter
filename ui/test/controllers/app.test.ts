// Role: test — controller behavior against fake backend, views, and beep. Covers
// every rendering rule in spec decision 2.

import { describe, expect, it } from 'vitest';

import type { CommandId, SessionEvent, SubmitAck } from '../../src/protocol';
import type { AnnouncerView } from '../../src/ports/announcer_view';
import type { BackendApi } from '../../src/ports/backend_api';
import type { BeepView } from '../../src/ports/beep_view';
import type { BufferView } from '../../src/ports/buffer_view';
import type { EditFieldView } from '../../src/ports/edit_field_view';
import {
  AppController,
  altScreenEnteredMessage,
  integrationUnavailableMessage,
  altScreenLeftMessage,
  commandStoppedMessage,
  failureMessage,
  outputContinuesMessage,
  patienceMessage,
  tooBigMessage,
} from '../../src/controllers/app';

class FakeBackend implements BackendApi {
  submitted: string[] = [];
  private nextId = 1;
  private onEvent: ((event: SessionEvent) => void) | undefined;

  attachSession(onEvent: (event: SessionEvent) => void): Promise<void> {
    this.onEvent = onEvent;
    return Promise.resolve();
  }
  submitCommand(line: string): Promise<SubmitAck> {
    this.submitted.push(line);
    return Promise.resolve({ command_id: this.nextId++ });
  }
  /** Push an event as the backend would over the Channel. */
  emit(event: SessionEvent): void {
    this.onEvent?.(event);
  }
}

class FakeEditField implements EditFieldView {
  text = '';
  focused = true;
  clearedCount = 0;
  value(): string {
    return this.text;
  }
  clear(): void {
    this.text = '';
    this.clearedCount += 1;
  }
  focus(): void {
    this.focused = true;
  }
  isFocused(): boolean {
    return this.focused;
  }
}

class FakeBuffer implements BufferView {
  opened: Array<{ commandId: CommandId; commandLine: string }> = [];
  appended: Array<{ commandId: CommandId; text: string }> = [];
  focused = false;
  openBlock(commandId: CommandId, commandLine: string): void {
    this.opened.push({ commandId, commandLine });
  }
  appendOutput(commandId: CommandId, text: string): void {
    this.appended.push({ commandId, text });
  }
  focus(): void {
    this.focused = true;
  }
  containsFocus(): boolean {
    return this.focused;
  }
}

class FakeAnnouncer implements AnnouncerView {
  announcements: string[] = [];
  announce(text: string): void {
    this.announcements.push(text);
  }
}

class FakeBeep implements BeepView {
  beeps = 0;
  beep(): void {
    this.beeps += 1;
  }
}

function makeApp() {
  const backend = new FakeBackend();
  const editField = new FakeEditField();
  const buffer = new FakeBuffer();
  const announcer = new FakeAnnouncer();
  const beep = new FakeBeep();
  const controller = new AppController(backend, editField, buffer, announcer, beep);
  return { backend, editField, buffer, announcer, beep, controller };
}

describe('submit', () => {
  it('submits trimmed text, opens the block tagged with the ack id, clears the field', async () => {
    const { backend, editField, buffer, controller } = makeApp();
    editField.text = '  small  ';

    await controller.submit();

    expect(backend.submitted).toEqual(['small']);
    expect(buffer.opened).toEqual([{ commandId: 1, commandLine: 'small' }]);
    expect(editField.clearedCount).toBe(1);
  });

  it('ignores empty and whitespace-only input', async () => {
    const { backend, buffer, editField, controller } = makeApp();
    editField.text = '   ';

    await controller.submit();

    expect(backend.submitted).toEqual([]);
    expect(buffer.opened).toEqual([]);
    expect(editField.clearedCount).toBe(0);
  });
});

describe('event rendering (decision 2)', () => {
  it('Output appends the text, and the ReadAloud about it speaks it', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello from acter' },
    });

    expect(buffer.appended).toEqual([{ commandId: 1, text: 'hello from acter' }]);
    expect(announcer.announcements).toEqual(['hello from acter']);
  });

  it('renders an auto-read chunk into the buffer before announcing it (A5.2)', async () => {
    const backend = new FakeBackend();
    const order: string[] = [];
    // Two fakes sharing one call log: the invariant under test is cross-view ordering,
    // which the per-view recordings cannot show.
    const buffer: BufferView = {
      openBlock: () => {},
      appendOutput: () => {
        order.push('buffer');
      },
      focus: () => {},
      containsFocus: () => false,
    };
    const announcer: AnnouncerView = {
      announce: () => {
        order.push('announce');
      },
    };
    const controller = new AppController(
      backend,
      new FakeEditField(),
      buffer,
      announcer,
      new FakeBeep(),
    );
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    // Two events now, in the order the backend sends them: A6 moved this invariant from
    // "append before announcing inside one handler" to "the render event before the
    // announce about it". The channel delivers in order, so obeying that order is enough.
    backend.emit({ type: 'Output', command_id: 1, text: 'hello' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello' },
    });

    expect(order).toEqual(['buffer', 'announce']);
  });

  it('a too-big chunk is appended whole and announced by its line count', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();
    const text = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join('\n');

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 40 },
    });

    expect(buffer.appended).toEqual([{ commandId: 1, text }]);
    expect(announcer.announcements).toEqual([tooBigMessage(40)]);
    expect(announcer.announcements[0]).toBe('40 lines arrived, too big to read');
  });

  it('an Output with no announcement after it appends and says nothing', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'still working' });

    expect(buffer.appended).toEqual([{ commandId: 1, text: 'still working' }]);
    expect(announcer.announcements).toEqual([]);
  });

  it('a successful fully-auto-read command gets no extra finish speech and no beep', async () => {
    const { backend, announcer, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello from acter' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(announcer.announcements).toEqual(['hello from acter']);
    expect(beep.beeps).toBe(0);
  });

  // The event shape the backend actually produces: the text is rendered quietly, the
  // speech about it rides an `Announce`, and the failure verdict is a second `Announce`
  // sent after the terminal event. Written this way because B6's manual NVDA pass heard
  // the failure spoken TWICE and heard it FIRST — before the error line it was about —
  // which is what a second producer in `CommandFinished` did.
  it('announces the output and then the failure, once each, in that order', async () => {
    const { backend, announcer, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'error: boom' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'error: boom' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'Failed', exit_code: 2 },
    });

    // The error text first, the verdict about it second, and each said exactly once.
    expect(announcer.announcements).toEqual(['error: boom', failureMessage(2)]);
    expect(beep.beeps).toBe(0);
  });

  // A finish carries no speech of its own: `CommandFinished` is a lifecycle event, and
  // everything spoken about a command comes from an `Announce`.
  it('a failing command that finishes says nothing until the Failed announcement', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(announcer.announcements).toEqual([]);
  });

  it('CommandInterrupted announces the stop and closes the block', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'phase one' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'phase one' },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });

    expect(announcer.announcements).toEqual(['phase one', commandStoppedMessage]);
    expect(announcer.announcements[1]).toBe('command stopped');
    // The block was closed, so a later event for the same id opens a fresh one.
    backend.emit({ type: 'Output', command_id: 1, text: 'late' });
    expect(buffer.opened).toEqual([
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: '' },
    ]);
  });

  it('does not beep on a stopped command that had carried a too-big chunk', async () => {
    const { backend, beep, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });

    // The beep answers "your too-big output finished"; a stop already has a spoken
    // answer, so it must stay silent (A3.1 decision 7).
    expect(beep.beeps).toBe(0);
    expect(announcer.announcements.at(-1)).toBe(commandStoppedMessage);
  });

  it('clears the too-big flag on interrupt, so a reused id does not beep later', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandInterrupted', command_id: 1 });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(0);
  });

  it('beeps on finish when an earlier chunk carried a too-big verdict', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'Output', command_id: 1, text: 'trickle' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'trickle' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(1);
  });

  // The remainder flushed at the end of a command can itself be too big. That verdict
  // used to ride `CommandFinished.read_mode`; since A6 it is an `Announce` the backend
  // sends before closing the block, and it must still arm the beep.
  it('beeps when the verdict on the final remainder is too-big', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 900 },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });

    expect(beep.beeps).toBe(1);
  });

  it('does not carry the too-big beep flag across commands', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    // Command 1 is too-big and beeps.
    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 2 },
    });
    backend.emit({ type: 'CommandFinished', command_id: 1 });
    // Command 2 is plain; it must not beep.
    backend.emit({ type: 'Output', command_id: 2, text: 'ok' });
    backend.emit({
      type: 'Announce',
      command_id: 2,
      announcement: { kind: 'ReadAloud', text: 'ok' },
    });
    backend.emit({ type: 'CommandFinished', command_id: 2 });

    expect(beep.beeps).toBe(1);
  });

  it('AltScreenEntered and AltScreenLeft announce the pinned strings', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'AltScreenEntered' });
    backend.emit({ type: 'AltScreenLeft' });

    expect(announcer.announcements).toEqual([
      altScreenEnteredMessage,
      altScreenLeftMessage,
    ]);
    expect(announcer.announcements[0]).toBe(
      'this program needs interactive mode, which is not available yet. Press Ctrl+C to return to the prompt',
    );
    expect(announcer.announcements[1]).toBe('interactive program ended');
  });

  it('IntegrationUnavailable announces the pinned string and opens no block', async () => {
    const { backend, announcer, buffer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'IntegrationUnavailable' });

    expect(announcer.announcements).toEqual([integrationUnavailableMessage]);
    expect(announcer.announcements[0]).toBe(
      'shell integration unavailable, output will not be read automatically; review it in the buffer',
    );
    expect(buffer.opened).toEqual([]);
  });

  it('TitleChanged and ConnectionChanged are silent no-ops', async () => {
    const { backend, announcer, buffer, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'TitleChanged', title: '~/acter' });
    backend.emit({ type: 'ConnectionChanged', state: 'Reconnecting' });

    expect(announcer.announcements).toEqual([]);
    expect(buffer.opened).toEqual([]);
    expect(beep.beeps).toBe(0);
  });

  it('lazily opens a block when an event arrives for an unsubmitted command', async () => {
    const { backend, buffer, controller } = makeApp();
    await controller.attach();

    // No submit happened; an Output races in first.
    backend.emit({ type: 'Output', command_id: 7, text: 'orphan chunk' });

    expect(buffer.opened).toEqual([{ commandId: 7, commandLine: '' }]);
    expect(buffer.appended).toEqual([{ commandId: 7, text: 'orphan chunk' }]);
  });

  it('sets the command line on the ack even when an event opened the block first', async () => {
    // The scripting race: CommandStarted/Output for command 1 arrives over the Channel
    // before the submit ack resolves, lazily opening the block with an empty heading.
    const { backend, buffer, editField, controller } = makeApp();
    await controller.attach();
    backend.emit({ type: 'CommandStarted', command_id: 1 });

    editField.text = 'small';
    await controller.submit();

    // The block was opened empty by the event, then the ack authoritatively set 'small'.
    expect(buffer.opened).toEqual([
      { commandId: 1, commandLine: '' },
      { commandId: 1, commandLine: 'small' },
    ]);
  });

  it('does not reopen a block already opened by submit', async () => {
    const { backend, buffer, editField, controller } = makeApp();
    await controller.attach();
    editField.text = 'small';
    await controller.submit();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter' });

    expect(buffer.opened).toEqual([{ commandId: 1, commandLine: 'small' }]);
  });
});

describe('focus flow', () => {
  it('F6 toggles from edit field to buffer and back', () => {
    const { editField, buffer, controller } = makeApp();

    editField.focused = true;
    controller.toggleFocusArea();
    expect(buffer.focused).toBe(true);

    editField.focused = false;
    controller.toggleFocusArea();
    expect(editField.focused).toBe(true);
  });

  it('Escape returns to the edit field only when the buffer has focus', () => {
    const { editField, buffer, controller } = makeApp();

    editField.focused = false;
    buffer.focused = false;
    controller.escapeToEditField();
    expect(editField.focused).toBe(false);

    buffer.focused = true;
    controller.escapeToEditField();
    expect(editField.focused).toBe(true);
  });
});

describe('Announce (B1.5): speech is its own event', () => {
  it('ReadAloud speaks the text without appending it — Output already did', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello\n' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'ReadAloud', text: 'hello\n' },
    });

    expect(buffer.appended).toEqual([{ commandId: 1, text: 'hello\n' }]);
    expect(announcer.announcements).toEqual(['hello\n']);
  });

  it('TooBig announces the count the backend sent, never a recount of the text', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    // Past the threshold the backend does not hold the text, so the count cannot be
    // re-derived here even in principle — it is carried.
    backend.emit({ type: 'Output', command_id: 1, text: 'y\ny\n' });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 500 },
    });

    expect(announcer.announcements).toEqual([tooBigMessage(500)]);
    expect(buffer.appended).toEqual([{ commandId: 1, text: 'y\ny\n' }]);
  });

  it('a too-big Announce arms the completion beep', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'TooBig', lines: 500 },
    });
    expect(beep.beeps).toBe(0);

    backend.emit({ type: 'CommandFinished', command_id: 1 });
    expect(beep.beeps).toBe(1);
  });

  it('StillRunning speaks the patience string', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'StillRunning' },
    });

    expect(announcer.announcements).toEqual([patienceMessage]);
  });

  it('OutputContinues says the output is still arriving, not that it stopped', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'OutputContinues' },
    });

    expect(announcer.announcements).toEqual([outputContinuesMessage]);
    expect(outputContinuesMessage).toContain('buffer');
  });

  it('Failed speaks the exit code', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'Failed', exit_code: 2 },
    });

    expect(announcer.announcements).toEqual([failureMessage(2)]);
  });

  it('quiet Output is buffered and silent, so the babble guard withholds nothing', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({
      type: 'Announce',
      command_id: 1,
      announcement: { kind: 'OutputContinues' },
    });
    for (const text of ['still\n', 'coming\n']) {
      backend.emit({ type: 'Output', command_id: 1, text });
    }

    expect(buffer.appended).toEqual([
      { commandId: 1, text: 'still\n' },
      { commandId: 1, text: 'coming\n' },
    ]);
    expect(announcer.announcements).toEqual([outputContinuesMessage]);
  });
});
