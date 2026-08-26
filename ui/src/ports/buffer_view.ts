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
  /**
   * Put a prompt the shell drew into the buffer, between blocks, as its own element.
   *
   * **Not a heading**, deliberately: heading navigation is how a listener walks
   * *commands*, and putting prompts into that sequence would double its length and break
   * the rhythm A5 and B4.4 were built for. It is readable where it happened, which is what
   * a listener reviewing the session needs (spec B5.6, decision 5).
   */
  appendPrompt(text: string): void;
  /**
   * Empty the buffer.
   *
   * **Called between one session ending and the next one being attached to** (spec B7,
   * decision 1), which is the moment the frontend gets to choose precisely because the
   * attach is a separate call. A buffer still holding one shell's output while another
   * one's arrives under it is a transcript of a session that never happened, and a
   * listener reviewing it by heading has no way to see the seam.
   */
  clear(): void;
  focus(): void;
  containsFocus(): boolean;
}
