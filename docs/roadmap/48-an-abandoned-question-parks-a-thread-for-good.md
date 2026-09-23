# 48 — A question nobody will answer parks a thread for good.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A question nobody will answer parks a thread for good.** Spec: none yet → specify first.

Found on 2026-09-23 while stripping comments for C9 (PR #69), by reading the code; not
yet reproduced in the running application.

`Connecting::begin` in `crates/acter-app/src/controllers/connecting.rs` runs
`ConnectService::use_profile` on Tauri's blocking pool (`spawn_blocking`), and the closure
holds the attempt's `Conversation`. When the attempt asks something,
`Conversation::ask` (`crates/acter-core/src/services/conversation.rs`) stores the answer
sender in `Conversation::waiting` and blocks on `receiver.recv()`, which becomes `GiveUp`
only when every sender is dropped. The sender lives inside the conversation, and the
closure keeps the conversation alive, so the sender is never dropped while the thread
waits. `Connecting::ended` removes the attempt from the map and nothing else.

So if nobody ever answers (the window is reloaded or closed while a host-key, password,
unverified-file or set-up dialog is open, for example), the blocking thread waits for the
life of the process: one parked thread per such attempt. Two comments said an unanswered
question would read as giving up once the window's channel closed. C9 corrected them.

This is not entry 27.2, which is about the SSH server's own login deadline running out
while a person is still answering.
