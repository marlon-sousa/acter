// Role: port (driven) — what the controller needs from the results buffer. Blocks are
// keyed by CommandId so interleaved output from concurrent commands lands under the
// right heading (events are demultiplexed by id).

import type { CommandId } from '../protocol';

export interface BufferView {
  /**
   * Open a command's block: an h2 heading holding the command line, with an empty
   * output region beneath it, keyed by `commandId`. Idempotent: if the block already
   * exists (an event opened it before the submit ack arrived), a non-empty
   * `commandLine` updates the heading and an empty one leaves it unchanged — so the
   * authoritative line from the ack always wins the race.
   */
  openBlock(commandId: CommandId, commandLine: string): void;
  /**
   * Append an output chunk under the block for `commandId`. The block must already be
   * open (the controller guarantees this, opening one lazily if an event races ahead).
   */
  appendOutput(commandId: CommandId, text: string): void;
  /**
   * Move focus into the buffer. Landing contract: focus the most recent command
   * heading (the newest end of the terminal history) so a screen reader lands on it
   * and re-evaluates its browse/focus mode; when the buffer is empty, fall back to the
   * region container. The specific landing element is view-adapter knowledge — the
   * controller only asks for focus.
   */
  focus(): void;
  containsFocus(): boolean;
}
