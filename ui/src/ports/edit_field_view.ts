// Role: port (driven) — what the controller needs from the command input.

export interface EditFieldView {
  value(): string;
  clear(): void;
  focus(): void;
  isFocused(): boolean;
  /** An input's selection is invisible to `window.getSelection()`. */
  hasSelection(): boolean;
}
