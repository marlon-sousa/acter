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
  altScreenLeftMessage,
  failureMessage,
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
  it('Output/Auto appends the text and announces it', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter', read_mode: 'Auto' });

    expect(buffer.appended).toEqual([{ commandId: 1, text: 'hello from acter' }]);
    expect(announcer.announcements).toEqual(['hello from acter']);
  });

  it('Output/TooBig appends the text and announces the line count phrasing', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();
    const text = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join('\n');

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text, read_mode: 'TooBig' });

    expect(buffer.appended).toEqual([{ commandId: 1, text }]);
    expect(announcer.announcements).toEqual([tooBigMessage(40)]);
    expect(announcer.announcements[0]).toBe('40 lines arrived, too big to read');
  });

  it('Output/Quiet appends the text but announces nothing', async () => {
    const { backend, buffer, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'still working', read_mode: 'Quiet' });

    expect(buffer.appended).toEqual([{ commandId: 1, text: 'still working' }]);
    expect(announcer.announcements).toEqual([]);
  });

  it('a successful fully-auto-read command gets no extra finish speech and no beep', async () => {
    const { backend, announcer, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter', read_mode: 'Auto' });
    backend.emit({ type: 'CommandFinished', command_id: 1, exit_code: 0, read_mode: 'Auto' });

    expect(announcer.announcements).toEqual(['hello from acter']);
    expect(beep.beeps).toBe(0);
  });

  it('announces the auto-read output and then the failure, in that order', async () => {
    const { backend, announcer, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'error: boom', read_mode: 'Auto' });
    backend.emit({ type: 'CommandFinished', command_id: 1, exit_code: 2, read_mode: 'Auto' });

    // Both are announced, output first — the announcer appends rather than replacing,
    // so the failure never clobbers the error output the user needs to hear.
    expect(announcer.announcements).toEqual(['error: boom', failureMessage(2)]);
    expect(announcer.announcements[1]).toBe('command failed, exit code 2');
    expect(beep.beeps).toBe(0);
  });

  it('beeps on finish when an earlier chunk carried a too-big verdict', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb', read_mode: 'TooBig' });
    backend.emit({ type: 'Output', command_id: 1, text: 'trickle', read_mode: 'Auto' });
    backend.emit({ type: 'CommandFinished', command_id: 1, exit_code: 0, read_mode: 'Auto' });

    expect(beep.beeps).toBe(1);
  });

  it('beeps when the finish verdict itself is too-big', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStarted', command_id: 1 });
    backend.emit({ type: 'CommandFinished', command_id: 1, exit_code: 0, read_mode: 'TooBig' });

    expect(beep.beeps).toBe(1);
  });

  it('does not carry the too-big beep flag across commands', async () => {
    const { backend, beep, controller } = makeApp();
    await controller.attach();

    // Command 1 is too-big and beeps.
    backend.emit({ type: 'Output', command_id: 1, text: 'a\nb', read_mode: 'TooBig' });
    backend.emit({ type: 'CommandFinished', command_id: 1, exit_code: 0, read_mode: 'Auto' });
    // Command 2 is plain; it must not beep.
    backend.emit({ type: 'Output', command_id: 2, text: 'ok', read_mode: 'Auto' });
    backend.emit({ type: 'CommandFinished', command_id: 2, exit_code: 0, read_mode: 'Auto' });

    expect(beep.beeps).toBe(1);
  });

  it('CommandStillRunning announces the patience string', async () => {
    const { backend, announcer, controller } = makeApp();
    await controller.attach();

    backend.emit({ type: 'CommandStillRunning', command_id: 1 });

    expect(announcer.announcements).toEqual([patienceMessage]);
    expect(announcer.announcements[0]).toBe(
      'long command running, output is accumulating in the buffer',
    );
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
    backend.emit({ type: 'Output', command_id: 7, text: 'orphan chunk', read_mode: 'Auto' });

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
    backend.emit({ type: 'Output', command_id: 1, text: 'hello from acter', read_mode: 'Auto' });

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
