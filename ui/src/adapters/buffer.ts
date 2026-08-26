// Role: adapter (DOM) — the results buffer region: one h2 per command keyed by
// CommandId, with output chunks appended under it as they arrive. A block for text no
// command accounts for has no h2 at all; see Block below.

import type { CommandId } from '../protocol';
import type { BufferView } from '../ports/buffer_view';

interface Block {
  // Absent until something says what this block is running. Text that belongs to no
  // command the user submitted — the shell's own prompt, its banner, the prompt a bare
  // Enter brings back — is a block with **no heading**, which is what DESIGN says it
  // gets. Rendering that as an *empty* heading is a different thing and a worse one: a
  // level 2 heading announcing nothing, which heading navigation lands on and cannot
  // read (found in B4.9's manual pass).
  heading: HTMLElement | null;
  output: HTMLElement;
}

export class BufferDom implements BufferView {
  // CommandId -> that command's heading and output container.
  private readonly blocks = new Map<CommandId, Block>();

  constructor(private readonly region: HTMLElement) {}

  /**
   * The buffer is in the document only while it has something in it (spec A10).
   *
   * An empty region is a thing a listener arrows onto and hears nothing useful from, and
   * since B7 an empty buffer is what every launch opens with rather than a state that lasts
   * until the first prompt arrives. Every method that puts something in calls this; only
   * `clear` takes it away again.
   */
  private show(): void {
    this.region.hidden = false;
  }

  appendPrompt(text: string): void {
    // A paragraph rather than a heading, and outside any block: the prompt belongs to the
    // gap between what just finished and what runs next, which is exactly where it is
    // drawn. Closing the current block first would be wrong — blocks are closed by the
    // shell, not by the buffer — so this is simply appended at the end of the region.
    const prompt = this.region.ownerDocument.createElement('p');
    prompt.className = 'prompt';
    prompt.textContent = text;
    this.region.append(prompt);
    this.show();
  }

  clear(): void {
    // The blocks map goes with the DOM. Leaving it behind would make the next session's
    // first command id — which starts again at 1 — find a block belonging to a shell that
    // is gone, and append its output under the previous shell's heading.
    this.region.replaceChildren();
    this.blocks.clear();
    this.region.hidden = true;
  }

  openBlock(commandId: CommandId, commandLine: string): void {
    // Idempotent. If the block already exists (an event opened it before the submit
    // ack arrived), a non-empty line updates its heading; an empty line leaves it be,
    // so the authoritative line from the ack wins the race and a later empty-line
    // event never clobbers it.
    const existing = this.blocks.get(commandId);
    if (existing !== undefined) {
      if (commandLine === '') {
        return;
      }
      // A block that opened with nothing to call it, now named: the heading is created
      // here and put in front of the output it belongs to, so the block reads in the
      // same order it would have had all along.
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
    this.blocks.set(commandId, { heading, output });
    this.show();
  }

  private newHeading(commandLine: string): HTMLElement {
    const heading = document.createElement('h2');
    heading.textContent = commandLine;
    // Programmatically focusable (a heading is never in the tab order) so focus()
    // can land here without adding it to sequential navigation.
    heading.tabIndex = -1;
    return heading;
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
