// Role: port (driven) — what the controller needs from the results buffer.

export interface BufferView {
  appendBlock(command: string, response: string): void;
  /**
   * Move focus into the buffer. Landing contract: focus the most recent command
   * heading (the newest end of the terminal history) so a screen reader lands on
   * it and re-evaluates its browse/focus mode; when the buffer is empty, fall
   * back to the region container. The specific landing element is view-adapter
   * knowledge — the controller only asks for focus.
   */
  focus(): void;
  containsFocus(): boolean;
}
