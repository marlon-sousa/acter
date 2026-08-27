#!/bin/sh
# Start sshd in the foreground, having first decided which identity this container has.
#
# `sh` rather than `bash`: this runs as PID 1 and does nothing bash is needed for. The
# *shell under test* is bash, and that is the login shell of the account Acter connects as.

set -eu

# **The changed-host-key case, on demand** (spec B9, decision 3). Host keys are baked at
# build time so the container has a stable identity and a `known_hosts` entry survives a
# restart; starting with ACTER_SSH_REKEY=1 throws that identity away and generates a new one,
# which is what a user meets when a server is rebuilt — or when somebody is between them and
# it. Refusing that connection is the default Acter has to get right, and this is how it is
# put in front of it.
if [ "${ACTER_SSH_REKEY:-0}" = "1" ]; then
    rm -f /etc/ssh/ssh_host_*
    ssh-keygen -A
    echo "acter-ssh: host keys regenerated; this container now has a NEW identity" >&2
fi

# **No attempt to set the hostname here, and that is a correction.** This script tried, with
# a `|| true` after it, and the try never succeeded: a container cannot rename itself without
# CAP_SYS_ADMIN, so the prompt read `acter@8bb4e345cb83` while the code claimed otherwise.
# A silent fallback that never fires is worse than no code at all, so the flag is in the
# documented `docker run` line instead — `--hostname acter-ssh` — where it works and where a
# reader can see it.

# What a listener will hear the first time, printed where a human running the container can
# see it — never over the connection, where it would be indistinguishable from output.
echo "acter-ssh: fingerprints of this container's host keys" >&2
for key in /etc/ssh/ssh_host_*_key.pub; do
    [ -e "$key" ] || continue
    ssh-keygen -l -f "$key" >&2
done

# `-D` keeps sshd in the foreground so the container's lifetime is the server's, and `-e`
# sends its log to stderr so `docker logs` is the whole story.
exec /usr/sbin/sshd -D -e
