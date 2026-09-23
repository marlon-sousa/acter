# 43 — A leftover answer can resolve the next question in the same attempt.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A leftover answer can resolve the next question in the same attempt.** Spec: none yet →
specify first.

Found on 2026-09-23 while stripping comments for C4 (PR #67), by reading the code; nothing
has been observed with a screen reader yet.

`Conversation::answer` in `crates/acter-core/src/services/conversation.rs` takes whatever
sender is waiting and sends the answer to it. It does not check that the answer is the
kind the waiting question asked for. The only filter is by attempt: `answer` in
`crates/acter-app/src/controllers/connecting.rs` looks the conversation up in
`HashMap<AttemptId, Arc<Conversation>>`. The doc comment on `answer`, deleted in C4, said a
stale answer is dropped, "a second click on a button that was already pressed" included.
That holds only when no question is waiting.

So within one attempt, an answer meant for a question that is gone reaches the next
question. The comment on `password`'s match names the case: `Trust` can only arrive there
as a stale answer to a host-key question. Every mismatched answer is read as a refusal
(`password` gives `None`, `host_key` gives `Refuse`, and so on), so it fails safe. The
failure a person would meet is a second click on Trust that lands after the password
dialog opened: the attempt ends as though they had declined to give a password.
