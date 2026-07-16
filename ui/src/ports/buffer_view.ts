// Role: port (driven) — what the controller needs from the results buffer.

export interface BufferView {
  appendBlock(command: string, response: string): void;
  focus(): void;
  containsFocus(): boolean;
}
