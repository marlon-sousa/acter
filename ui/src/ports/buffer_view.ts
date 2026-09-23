// Role: port (driven) — what the controller needs from the results buffer.

import type { CommandId, LineId, LineRevision } from '../protocol';

export interface BufferView {
  openBlock(commandId: CommandId, commandLine: string): void;
  /** The block for `commandId` must already be open. */
  applyLine(
    commandId: CommandId,
    line: LineId,
    revision: LineRevision,
    text: string,
  ): void;
  appendPrompt(text: string): void;
  clear(): void;
  focus(): void;
  containsFocus(): boolean;
}
