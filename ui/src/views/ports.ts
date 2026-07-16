// Role: ports (driven) — what the controller needs from the DOM world.

export interface EditFieldView {
  value(): string;
  clear(): void;
  focus(): void;
  isFocused(): boolean;
}

export interface BufferView {
  appendBlock(command: string, response: string): void;
  focus(): void;
  containsFocus(): boolean;
}

export interface AnnouncerView {
  announce(text: string): void;
}
