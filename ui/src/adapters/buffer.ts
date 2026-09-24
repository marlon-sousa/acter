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
      } else {
        existing.heading.textContent = commandLine;
      }
      markEcho(existing.heading);
      return;
    }

    const heading = commandLine === '' ? null : this.newHeading(commandLine);

    const output = document.createElement('div');
    output.className = 'response';

    this.region.append(...(heading === null ? [output] : [heading, output]));
    this.blocks.set(commandId, { heading, output, lines: new Map() });
    if (heading !== null) {
      markEcho(heading);
    }
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
    prompt = false,
  ): void {
    const block = this.blocks.get(commandId);
    if (block === undefined) {
      // Output for a block that was never opened is dropped.
      return;
    }
    let row = block.lines.get(line);
    if (row === undefined) {
      row = document.createElement('div');
      row.textContent = text;
      block.output.append(row);
      block.lines.set(line, row);
    } else if (revision === 'Appended') {
      row.textContent = `${row.textContent ?? ''}${text}`;
    } else {
      row.textContent = text;
    }
    row.classList.toggle('prompt-row', prompt);
    const next = block.output.nextElementSibling;
    if (next instanceof HTMLElement && next.tagName === 'H2') {
      markEcho(next);
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

// Marks whichever of a heading and the line above it repeats the other; the stylesheet hides it from sight only.
function markEcho(heading: HTMLElement): void {
  const before = heading.previousElementSibling;
  const above = before?.classList.contains('response') ? before.lastElementChild : before;
  const text = heading.textContent?.trimEnd() ?? '';
  const line = above?.textContent?.trimEnd() ?? '';
  const echoed =
    text !== '' &&
    line.length > text.length &&
    line.endsWith(text) &&
    /\s/.test(line.charAt(line.length - text.length - 1));
  heading.classList.toggle('echoed', echoed);
  if (above?.classList.contains('prompt-row') === true) {
    above.classList.toggle('repeated', line !== '' && text.startsWith(line));
  }
}
