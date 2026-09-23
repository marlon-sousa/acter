// Role: port (driving, view) — whoever can put a question from a connection in front of a
// person and come back with their answer.

import type { ConnectAnswer, ConnectQuestion } from '../protocol';

export interface QuestionView {
  /** Never rejects: closing the dialog answers that the person gave up. */
  ask(question: ConnectQuestion): Promise<ConnectAnswer>;
}
