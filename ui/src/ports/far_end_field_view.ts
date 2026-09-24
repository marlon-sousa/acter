// Role: port (driven) — what the controller needs from the far end's command line.

export interface FarEndFieldView {
  /**
   * `text` is `null` when only the caret moved. `caret` counts characters from the start of
   * the row. NVDA does not speak after `Tab`, so `completed` marks a row that answers one.
   */
  render(text: string | null, caret: number, anchored: boolean, completed?: boolean): void;
  show(showing: boolean): void;
  focus(): void;
  isFocused(): boolean;
}
