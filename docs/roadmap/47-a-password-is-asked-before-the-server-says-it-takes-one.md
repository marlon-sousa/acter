# 47 — A password is asked for before the server says it takes one.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A password is asked for before the server says it takes one.** Spec: none yet → specify
first.

Found on 2026-09-23 while stripping comments for C7 (PR #69), by reading the code. The
spec's list of stale comments had already named it.

`authenticate` in `crates/acter-transports/src/ssh/transport.rs` tells the listener
"Signing in." and then, on the first pass of its loop, asks `SshQuestions::password`
without asking the server which methods it offers. `offers_password` is consulted only
after `authenticate_password` has been rejected, to decide whether to ask again. The doc
comment removed in C7 said the password is "asked only when the server will take one",
which is not what the code does.

On a server that does not offer password authentication (keys only, for example), the
person is asked for a password, types one, and only then hears that the server "would not
accept that password … and will not accept another one".
