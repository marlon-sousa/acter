// Role: port (driven) — what the controller needs from the command input.

export interface EditFieldView {
  value(): string;
  clear(): void;
  focus(): void;
  isFocused(): boolean;
  /**
   * Whether the field currently holds a selected range. `Ctrl+C` over a selection is
   * the native copy and must never reach the backend (DESIGN's keystroke map, layer 2),
   * so the controller asks before reporting the key. Asked of the field rather than of
   * the document because an input's selection is invisible to `window.getSelection()`.
   */
  hasSelection(): boolean;
}
