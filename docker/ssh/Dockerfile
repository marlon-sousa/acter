# A far end reached over SSH, for B9 to be measured against.
#
# **Why a container rather than sshd on this machine.** Enabling the Windows OpenSSH server
# opens a listening service on a laptop that joins other networks, which is a real security
# decision and not a development convenience. A container is disposable, binds to loopback
# only, and — the property that actually matters for this project — is genuinely **not in
# Acter's process tree**, which is the thing 22.6 established changes how a far end behaves.
#
# **Debian, and bash, deliberately.** B5.3 measured the shell-integration injection against
# bash inside a WSL distribution; using the same shell here means a difference between the
# two is a fact about SSH rather than a fact about the distribution. Alpine would give
# busybox `ash` and prove nothing about markers.
#
# **The default `.bashrc` is part of the test, not an accident.** `useradd -m` copies
# Debian's own `/etc/skel/.bashrc`, which sets `PS1`, defines aliases and returns early when
# the shell is not interactive. Since decision 2 leaves an SSH session unintegrated, the
# prompt that `.bashrc` draws is exactly what a listener hears and what Acter has to render
# without marker help — and it is the file the deferred install-snippet entry would modify.
# A container with an empty one would answer a question nobody asked.

FROM debian:bookworm-slim

# `openssh-server` is the point. `procps` and `iputils-ping` are here so an interrupt can be
# measured against something that genuinely runs for a while (spec B9's definition of done:
# "interrupt and resize cross the connection"), rather than against a command that has
# finished before the signal arrives.
#
# **`zsh` is here for B5.8**, which measures a setup for it. A second real shell at the far
# end is what turns "one mechanism for every shell" from a claim into a measurement: zsh's
# setup is not bash's with a substitution, and only a far end that runs it can show that.
RUN apt-get update \
    && apt-get install --no-install-recommends --yes \
        openssh-server \
        procps \
        iputils-ping \
        zsh \
    && rm -rf /var/lib/apt/lists/*

# sshd refuses to start without this directory, and the package does not always create it.
RUN mkdir -p /run/sshd

# **The hostname is a run-time flag, not something set here** — measured 2026-08-26. It
# becomes the second half of bash's default prompt (`acter@acter-ssh:~$`), which is what a
# listener hears when the far end draws it; a container id makes every measurement read
# differently. A container cannot rename itself without CAP_SYS_ADMIN, so `--hostname` on the
# `docker run` line is the only thing that works. See the README.

# The account Acter connects as. The password is weak on purpose and is safe only because of
# how this is run: loopback-only publishing, no port reachable from outside the machine. It
# is never a template for anything shipped.
ARG SSH_USER=acter
ARG SSH_PASSWORD=acter
ENV SSH_USER=${SSH_USER}

RUN useradd --create-home --shell /bin/bash "${SSH_USER}" \
    && echo "${SSH_USER}:${SSH_PASSWORD}" | chpasswd \
    && mkdir -p "/home/${SSH_USER}/.ssh" \
    && chmod 700 "/home/${SSH_USER}/.ssh" \
    && chown -R "${SSH_USER}:${SSH_USER}" "/home/${SSH_USER}/.ssh"

# **A second account whose login shell is not bash, and it is the control for decision 7.**
#
# The probe reads `$SHELL` on an `exec` channel to find out what the far end runs. Against
# the `acter` account that proves less than it looks: **bash sets `$SHELL` itself when it is
# unset**, so a value coming back could have been bash's own invention rather than sshd's
# answer from the account's passwd entry. `dash` has no such behaviour, so a correct value
# arriving for *this* account can only have come from sshd — which is what makes the probe
# trustworthy for the accounts it exists to serve rather than only for the one that would
# have flattered it.
#
# Measured 2026-08-26 with the account added by hand; it is in the image now so the
# measurement can be repeated by anybody who builds the rig, which is the whole point of
# having a rig in the repository.
ARG DASH_USER=dashuser
RUN useradd --create-home --shell /bin/dash "${DASH_USER}" \
    && echo "${DASH_USER}:${SSH_PASSWORD}" | chpasswd

# **A third account whose login shell is zsh, and it is what B5.8 is measured against.**
#
# zsh is the second shell with a measured setup, and the reason it needed one is that its
# setup is not bash's with a substitution: the verdict is a prompt escape the shell expands
# for itself, the `C` comes from a real `preexec` hook rather than a `DEBUG` trap, and the
# user's own hooks are appended to in an array rather than captured out of a string. None of
# that is visible without a far end that actually runs it.
#
# **The `.zshrc` is here because of what happened without one, measured 2026-08-31.** An
# account with no zsh startup files at all does not get a prompt: it gets
# `zsh-newuser-install`, an interactive menu that reads a single keypress. Acter's setup line
# went out into that menu, its leading `p` was consumed as the answer, the menu aborted, and
# the remainder reached the shell as `rint -n ...` — `zsh: command not found`. The session
# still ended up integrated, because everything after the first `;` ran, which is a piece of
# luck and not a design. That case is real and belongs to a listener, so it is filed as a
# roadmap entry rather than hidden; what it is not is *this* account's subject, and a rig
# that meets a first-run wizard measures the wizard.
#
# So this account has the file every account that has ever run zsh has, and it does the one
# thing Debian's own `zsh-newuser-install` offers to do for you.
ARG ZSH_USER=zshuser
RUN useradd --create-home --shell /bin/zsh "${ZSH_USER}" \
    && echo "${ZSH_USER}:${SSH_PASSWORD}" | chpasswd \
    && printf '%s\n' '# Left minimal on purpose: this account exists to run zsh, not a theme.' > "/home/${ZSH_USER}/.zshrc" \
    && chown "${ZSH_USER}:${ZSH_USER}" "/home/${ZSH_USER}/.zshrc"

# A public key baked in at build time, for testing key authentication without mounting
# anything. Optional: left empty, only password authentication works.
#   docker build --build-arg "SSH_PUBKEY=$(cat ~/.ssh/id_ed25519.pub)" ...
ARG SSH_PUBKEY=""
RUN if [ -n "${SSH_PUBKEY}" ]; then \
        echo "${SSH_PUBKEY}" > "/home/${SSH_USER}/.ssh/authorized_keys" \
        && chmod 600 "/home/${SSH_USER}/.ssh/authorized_keys" \
        && chown "${SSH_USER}:${SSH_USER}" "/home/${SSH_USER}/.ssh/authorized_keys"; \
    fi

COPY sshd_config /etc/ssh/sshd_config.d/acter.conf
COPY entrypoint.sh /usr/local/bin/acter-ssh-entrypoint
RUN chmod 755 /usr/local/bin/acter-ssh-entrypoint

# **Host keys are generated at build, not at start, and that is the whole point.** Baked
# keys give the container a *stable* identity across runs, so a `known_hosts` entry written
# on the first connection still matches on the second — which is what "a known host connects
# without asking" needs in order to be testable at all.
#
# The other half of spec B9's decision 3 — a host key that has **changed**, which is the
# security case and where refusal is the default — is produced on demand by starting the
# container with `ACTER_SSH_REKEY=1`. Two `docker run` lines, two states, no editing.
RUN ssh-keygen -A

EXPOSE 22
ENTRYPOINT ["/usr/local/bin/acter-ssh-entrypoint"]
