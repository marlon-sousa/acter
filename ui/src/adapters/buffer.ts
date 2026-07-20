// Role: adapter (DOM) — the results buffer region: one h2 per command keyed by
// CommandId, with output chunks appended under it as they arrive.

import type { CommandId } from '../protocol';
import type { BufferView } from '../ports/buffer_view';

interface Block {
  heading: HTMLElement;
  output: HTMLElement;
}

export class BufferDom implements BufferView {
  // CommandId -> that command's heading and output container.
  private readonly blocks = new Map<CommandId, Block>();

  constructor(private readonly region: HTMLElement) {}

  openBlock(commandId: CommandId, commandLine: string): void {
    // Idempotent. If the block already exists (an event opened it before the submit
    // ack arrived), a non-empty line updates its heading; an empty line leaves it be,
    // so the authoritative line from the ack wins the race and a later empty-line
    // event never clobbers it.
    const existing = this.blocks.get(commandId);
    if (existing !== undefined) {
      if (commandLine !== '') {
        existing.heading.textContent = commandLine;
      }
      return;
    }

    const heading = document.createElement('h2');
    heading.textContent = commandLine;
    // Programmatically focusable (a heading is never in the tab order) so focus()
    // can land here without adding it to sequential navigation.
    heading.tabIndex = -1;

    const output = document.createElement('div');
    output.className = 'response';

    this.region.append(heading, output);
    this.blocks.set(commandId, { heading, output });
  }

  appendOutput(commandId: CommandId, text: string): void {
    const block = this.blocks.get(commandId);
    if (block === undefined) {
      // The controller opens a block before appending; this guard keeps a scripting
      // race from throwing rather than silently losing output.
      return;
    }
    const chunk = document.createElement('div');
    chunk.textContent = text;
    block.output.append(chunk);
  }

  focus(): void {
    const headings = this.region.querySelectorAll('h2');
    const mostRecent = headings[headings.length - 1];
    (mostRecent ?? this.region).focus();
  }

  containsFocus(): boolean {
    return this.region.contains(document.activeElement);
  }
}
