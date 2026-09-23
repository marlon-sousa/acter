// Role: adapter (DOM) — the results buffer region: one h2 per command keyed by CommandId.

import type { CommandId, LineId, LineRevision } from '../protocol';
import type { BufferView } from '../ports/buffer_view';

interface Block {
  // Null while nothing says what the block runs; never an empty h2, which heading navigation
  // lands on and cannot read.
  heading: HTMLElement | null;
  output: HTMLElement;
  lines: Map<LineId, HTMLElement>;
}

export class BufferDom implements BufferView {
  private readonly blocks = new Map<CommandId, Block>();

  constructor(private readonly region: HTMLElement) {}

  // Every method that puts something in the region must call this; only `clear` hides it.
  private show(): void {
    this.region.hidden = false;
  }

  appendPrompt(text: string): void {
    const prompt = this.region.ownerDocument.createElement('p');
    prompt.className = 'prompt';
    prompt.textContent = text;
    this.region.append(prompt);
    this.show();
  }

  clear(): void {
    // The map must go with the DOM: the next session's command ids start again at 1.
    this.region.replaceChildren();
    this.blocks.clear();
    this.region.hidden = true;
  }

  openBlock(commandId: CommandId, commandLine: string): void {
    // An empty line never overwrites a heading, so the submit ack's line wins a race with an event.
    const existing = this.blocks.get(commandId);
    if (existing !== undefined) {
      if (commandLine === '') {
        return;
      }
      if (existing.heading === null) {
        existing.heading = this.newHeading(commandLine);
        existing.output.before(existing.heading);
        return;
      }
      existing.heading.textContent = commandLine;
      return;
    }

    const heading = commandLine === '' ? null : this.newHeading(commandLine);

    const output = document.createElement('div');
    output.className = 'response';

    this.region.append(...(heading === null ? [output] : [heading, output]));
    this.blocks.set(commandId, { heading, output, lines: new Map() });
    this.show();
  }

  private newHeading(commandLine: string): HTMLElement {
    const heading = document.createElement('h2');
    heading.textContent = commandLine;
    heading.tabIndex = -1;
    return heading;
  }

  applyLine(
    commandId: CommandId,
    line: LineId,
    revision: LineRevision,
    text: string,
  ): void {
    const block = this.blocks.get(commandId);
    if (block === undefined) {
      // Output for a block that was never opened is dropped.
      return;
    }
    const existing = block.lines.get(line);
    if (existing === undefined) {
      const row = document.createElement('div');
      row.textContent = text;
      block.output.append(row);
      block.lines.set(line, row);
    } else if (revision === 'Appended') {
      existing.textContent = `${existing.textContent ?? ''}${text}`;
    } else {
      existing.textContent = text;
    }
    this.show();
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
