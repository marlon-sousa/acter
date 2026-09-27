# A test build of a pull request, on request

Agreed in conversation on 2026-09-26, before the 0.1 beta (roadmap entry 55). The user wants
to hand a pull request to someone who has no development environment: they type a command in
a comment and, for seven days, a link gives them a copy of Acter built from that pull request.

A test build is not a beta. Betas are releases: tagged, on the releases page, built by
`release.yml`. A test build is a workflow artifact that expires, and nothing on the releases
page changes.

## What someone does and what they get

1. **The command is `@build-<platform>`, written anywhere in a new comment on a pull request.**
   Only `@build-windows` is supported. Any other platform, a comment on an issue, or a comment
   that is edited later to add the command does nothing: no reply, no reaction.
2. **The build is of the pull request's latest commit when the comment arrives.** A push while
   it builds does not change what is built.
3. **One build per commit while it lasts.** If the commit already has a test build that has
   not expired, the command does nothing. After the seven days, the same command builds it
   again. A new push is a new commit and gets its own build.
4. **The link is added to the same comment.** The workflow puts an "eyes" reaction on the
   comment when it starts, and when it finishes it appends a section to that comment: a
   heading naming the platform and the short commit, then a list with the download link and
   the date it stops working, how to run it, what Windows may say about an unknown publisher,
   and a link to the run. Each section starts with a hidden `<!-- build-windows:<sha> -->`
   marker. If the build fails, the section says so and links the run instead.
5. **The download is the portable zip.** GitHub serves an artifact as a zip, so the workflow
   uploads `acter.exe` alone and the download is a zip holding `acter.exe` and nothing else,
   the same content as a release's portable zip. Downloading needs a GitHub account; anyone
   who could type the command has one.
6. **The copy says it is a development build of that commit.** The build sets both
   `VERGEN_GIT_DESCRIBE` and `VERGEN_GIT_SHA` to the short commit, so `container::version`
   finds no release tag and About says "Development build, commit abc1234." No Rust changes.

## Who can ask, and what is safe

7. **Anyone who can comment can ask for a build of this repository's branches.** Only
   collaborators can push a branch here, so the code is trusted even when the person asking
   is not.
8. **A fork's pull request is built when the owner or a collaborator asks, or when its author
   asks and CI has been allowed to run on that commit.** A comment is never held for "Approve
   and run", so the build follows that approval instead: the author's request is honoured when
   `ci.yml` has a `pull_request` run for the commit that is not waiting for approval. That is
   either the owner's "Approve and run" or the repository's policy not asking about this
   contributor. A new push is a new commit and needs CI's approval again. Anyone else's
   `@build-windows` on a fork does nothing.
9. **Three jobs with separate permissions.** `decide` reads the comment, checks the pull
   request's origin, picks the commit, checks for an unexpired build and adds the reaction.
   `build` runs on Windows with read-only access. `answer` is the only job that edits the
   comment.
10. **Two requests for one commit do not build twice.** The build job holds a concurrency
    group named after the commit, and checks again for a build once it has the group, so a
    request that waited behind another finds that build and does nothing.
11. **Fork code never reaches a cache a release reads.** A fork's build restores the Rust
    cache but saves nothing, and uses no npm cache. Code running on the runner can reach the
    cache's credentials whatever the workflow says, so the release builds with no cache at
    all: what the public installs is always built from a clean tree.

## One build recipe

12. **The portable build is one composite step, `.github/actions/portable-build`,** used by
    both `release.yml` and `test-build.yml`, so a test build is what a release would ship. It
    installs the toolchains, builds the frontend and runs the `portable` cargo build into
    `target/portable`. Its `cache` input is `full`, `restore` or `off`. The test build takes it
    from main, not from the pull request, so a branch cut before this PR can still be built.

## Files touched

- `.github/workflows/test-build.yml`, new.
- `.github/actions/portable-build/action.yml`, new.
- `.github/workflows/release.yml`, uses the composite step for its portable binary, with no cache.
- This spec.

## Definition of done

- The composite step and both workflows pass `actionlint`.
- A local build with the two variables set to a short commit, and the `portable` feature,
  produces an `acter.exe` whose About says "Development build, commit" and that commit.
- After merge, which `issue_comment` requires before it runs at all, `@build-windows` on an
  open pull request from this repository adds a working link to that comment, the zip holds
  `acter.exe`, and a second `@build-windows` on the same commit changes nothing. On a fork's
  pull request, the owner's `@build-windows` builds it, and its author's builds it once CI
  has been approved for that commit. These run after the merge and are
  recorded on the PR.
