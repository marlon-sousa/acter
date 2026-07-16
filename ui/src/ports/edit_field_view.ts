// Role: port (driven) — what the controller needs from the command input.

export interface EditFieldView {
  value(): string;
  clear(): void;
  focus(): void;
  isFocused(): boolean;
}
