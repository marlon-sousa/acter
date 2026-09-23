// Role: port (driving, view) — something that can put one sentence in front of a person
// and wait for them to acknowledge it.

export interface MessageView {
  /** Resolves once the person has dismissed it. */
  show(sentence: string): Promise<void>;
}
