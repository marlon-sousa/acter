# A far end reached over SSH

The test rig B9 is measured against: a Debian container running `sshd`, with bash and its
own default `.bashrc` at the far end.

Why a container rather than `sshd` on this machine, why Debian, and why the `.bashrc` matters
are all in the [Dockerfile](Dockerfile); why `AcceptEnv` is deliberately *not* configured is
in [sshd_config](sshd_config) and is the single most consequential thing here.

## Build

```
docker build -t acter-ssh docker/ssh
```

With a key baked in, so key authentication can be measured without mounting anything:

```
docker build -t acter-ssh --build-arg "SSH_PUBKEY=$(cat ~/.ssh/id_ed25519.pub)" docker/ssh
```

## Run

**Loopback only.** The published port is bound to `127.0.0.1`, so nothing on any network
this machine joins can reach it. The password is weak and is safe only because of that; it
is never a template for anything shipped.

```
docker run --rm -d --name acter-ssh --hostname acter-ssh -p 127.0.0.1:2222:22 acter-ssh
```

`--hostname` is not optional decoration: bash's default prompt is `user@host`, so without it
every run draws a different prompt and no measurement of prompt text repeats. A container
cannot set its own hostname, so the flag is the only place this can live.

Then, from Windows: `ssh -p 2222 acter@127.0.0.1`, password `acter`.

The container prints its host-key fingerprints to `docker logs acter-ssh` when it starts,
which is what a user would be shown by a colleague or a hosting provider — and what a
listener has to be able to compare against when Acter asks them to accept an unknown key.

## The three host-key states, which are the point of the rig

Spec B9, decision 3 makes host key verification a dialog whose default is refusal. All three
cases it has to handle are reachable here, with no editing:

- **Unknown** — a fresh `known_hosts`, or a fresh image. Acter must ask.
- **Known** — connect twice to the same running container. Acter must not ask again.
- **Changed** — restart with `ACTER_SSH_REKEY=1`, which throws the identity away and
  generates a new one:

  ```
  docker rm -f acter-ssh
  docker run --rm -d --name acter-ssh --hostname acter-ssh -p 127.0.0.1:2222:22 -e ACTER_SSH_REKEY=1 acter-ssh
  ```

  This is the security case. What a listener hears here, and how hard it is to say yes by
  accident, is the most important thing in B9's accessibility checklist.

## What is deliberately absent

- **No `docker-compose`.** Two `docker run` lines that differ by one variable are easier to
  read aloud than a YAML file, and the difference between them *is* the test.
- **No agent, no jump host, no second hop.** Those are open questions in the spec rather than
  decided scope; the rig gains them when the spec does.
- **No permissive `AcceptEnv`.** See [sshd_config](sshd_config). A rig more accommodating
  than a real server would let B9 ship an injection that works nowhere else.
