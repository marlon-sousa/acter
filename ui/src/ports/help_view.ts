// Role: port (driving, view) — whoever can put the help topic in front of a person, at a
// named place in it.

export interface HelpView {
  /** `topic` is the id of a heading inside it; `returnTo` takes focus when it closes. */
  open(options?: { topic?: string; returnTo?: { focus(): void } }): void;
}
