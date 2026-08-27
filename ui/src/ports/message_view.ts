// Role: port (driving, view) — something that can put one sentence in front of a person
// and wait for them to acknowledge it.
//
// Separate from `QuestionView` because it asks nothing: there is no answer to route back,
// only a fact that must not be missed. The controller owns *what* is said — every one of
// those sentences is the backend's own — and knows nothing about dialogs.

export interface MessageView {
  /** Say it, and resolve once the person has dismissed it. */
  show(sentence: string): Promise<void>;
}
