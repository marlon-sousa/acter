// Role: port (driving, view) — whoever can put a question from a connection in front of a
// person and come back with their answer.
//
// **The controller owns the connection and knows nothing about dialogs**, which is what
// keeps the two host-key sentences and the masked field testable on their own, and what
// lets every existing test of the controller carry on with nobody to ask. A controller with
// no asker is not a controller that trusts everything: it answers that nobody answered, and
// the backend reads that as a refusal (spec B9, decision 3).

import type { ConnectAnswer, ConnectQuestion } from '../protocol';

export interface QuestionView {
  /**
   * Put this question to the user, and resolve with what they decided.
   *
   * Never rejects: a person who closes a dialog has decided something, and what they
   * decided is to give up.
   */
  ask(question: ConnectQuestion): Promise<ConnectAnswer>;
}
