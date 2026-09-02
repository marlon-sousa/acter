// Role: port (driven) — what the controller needs from the far end's command line.
//
// It is a separate port from `EditFieldView` because it is a separate thing, and the
// difference is the whole of DESIGN's "Edit field ownership": the edit field *holds* a line
// the user is composing and answers what is in it, while this one holds nothing of its own
// and renders what the far end drew. A single port with both surfaces would be the mirroring
// that section rejects wholesale, expressed as a type.

export interface FarEndFieldView {
  /**
   * Show the far end's row and put the caret in it.
   *
   * `text` is `null` when nothing was redrawn and only the caret moved, which is what left,
   * right, Home and End do — writing the row back unchanged would be a text change the
   * reader announces as one.
   *
   * `caret` counts characters from the start of the row the field holds.
   */
  render(text: string | null, caret: number): void;
  /** Whether the window shows the far end's line at all. */
  show(showing: boolean): void;
  focus(): void;
  isFocused(): boolean;
}
