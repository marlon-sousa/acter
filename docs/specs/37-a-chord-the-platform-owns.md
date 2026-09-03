# Spec: PR 37 — a chord the platform owns is never a keystroke for the far end

While the far end owns the line, `Cmd+C` types a `c` into the remote command line instead of
copying, and `Cmd+K` types a `k` instead of opening Connect. This makes the frontend leave
every chord carrying the platform's own modifier alone — unsent and, just as importantly,
unprevented — so the platform's accelerator fires and the menu item runs.

## Why now / relation to the roadmap

- Roadmap entry 37, lane 3, raised from a VoiceOver session on 2026-09-03. It is small,
  self-contained, and it is the first thing to fix in that lane: a listener pressing their
  platform's own copy command and finding a stray character in their shell is worse than
  silence, because the shell will run it.
- **It is not roadmap 36**, the entry this session's larger finding belongs to. That one is
  about what VoiceOver announces and needs a spec of its own; this one is about which keys
  leave the page, and the two share no code.
- **It contradicts something DESIGN already decided**, which is what makes it a defect rather
  than a gap: M3 put an Edit menu in the macOS bar on the reasoning that "macOS routes a
  webview's own copy and paste through the menu bar, so a bar without it takes `Cmd+C` out of
  the command line". The menu is there; the keystroke never reaches it.

## What was measured

VoiceOver 15.0, macOS 15.0, silent capture, `user` persona, 2026-09-03, in a real session
with the far end holding the line — a `bash` over SSH to the `docker/ssh` rig:

- `Cmd+K` did not open the Connect dialog. The far end's command line, read back with `VO-F4`,
  had gained a `k`.
- `Cmd+C` did not copy. The line had gained a `c`.
- The line then ran as `abcdefkc`, and `bash` answered `command not found`.

The same two chords work in local-line mode, which is why M3's checklist passed them: the
far-end listener is not in that path.

## Design decisions this spec makes

1. **The rule is about a modifier, not about a list of shortcuts.** Any chord carrying the
   platform's own modifier — `event.metaKey`, which is Command on macOS and the Windows key on
   Windows — is not a keystroke for the far end. Enumerating `Cmd+K`, `Cmd+C`, `Cmd+/` would
   leave `Cmd+W`, `Cmd+Q`, `Cmd+M`, `Cmd+H` and every accelerator a future menu grows still
   broken, and would need editing every time the menu does.

2. **Such a chord is not prevented either, and that half is the actual fix.** Returning
   without sending would stop the character reaching the far end and still swallow the
   accelerator, so `Cmd+K` would do nothing at all — a different defect wearing the same
   clothes. The listener returns *before* `preventDefault`, which is the same shape
   `keyOf`'s unmeasured keys already use: "it is not prevented either, so anything the
   platform still owns keeps working".

3. **The rule is stated once and used everywhere the frontend claims a key.** One predicate,
   applied in the far-end listener, in layer 1's toggle and in the edit field's two reported
   keys. Layer 1 keeps its exact spelling: `Ctrl+Shift+K` is the toggle and
   `Cmd+Ctrl+Shift+K` is not, so a future platform chord that happens to contain layer 1's
   letters is the platform's rather than silently Acter's.

4. **It is not platform-dependent, and deliberately not written as if it were.** The Windows
   key is the platform's on Windows exactly as Command is on macOS, and neither is a modifier
   a terminal carries. So there is no `os` argument here, no `data-platform`, and no second
   adapter — this rule is one condition that is true everywhere, which is the smallest honest
   shape it has.

5. **`Alt` stays the far end's, and the word "Meta" is the trap.** In terminal vocabulary
   Meta *is* Alt — DESIGN's layer 3 says "Alt combos (Meta keys)" and means `ESC`-prefixed
   sequences that a far end genuinely reads. In DOM vocabulary `metaKey` is the Command or
   Windows key, which no terminal has ever received. This spec means the second, and says so
   here because the two readings of one word sit three lines apart in the same file.

6. **`Ctrl` stays the far end's too**, unchanged: plain `Ctrl+C` in far-end-line mode is the
   interrupt by another road (DESIGN's layer 2 amendment, 2026-08-31), and this spec touches
   nothing about it.

7. **No announcement of its own.** A chord that reaches the platform is answered by the
   platform — the menu opens, the clipboard fills, the reader says so. Acter saying anything
   extra would be narrating somebody else's work.

## Files touched

- `ui/src/adapters/keyboard.ts` — the predicate and its three uses.
- `ui/test/adapters/keyboard.test.ts` — unit tests for both platforms' spellings.
- `docs/DESIGN.md` — the three-layer rule gains the sentence, because "which modifiers are the
  platform's" is a product decision rather than an implementation detail.
- `docs/ROADMAP.md` — entry 37 flips to Done.
- `docs/specs/37-a-chord-the-platform-owns.md` — this file.

## Definition of done

1. A `Cmd+`letter chord pressed at the far-end field reports **no** keystroke and is **not**
   prevented.
2. The same chord at the edit field in local-line mode reports nothing — including
   `Cmd+C` and `Cmd+D`, whose unmodified `Ctrl` spellings are the two keys that field
   forwards.
3. `Ctrl+Shift+K` still toggles the line owner; `Cmd+Ctrl+Shift+K` does not, and is not
   prevented.
4. `Ctrl+C`, `Ctrl+D`, the arrows, `Tab`, `Enter`, `Backspace` and ordinary characters are
   unchanged in both modes: this spec adds a condition and removes no behaviour.
5. `npm -w acter-ui test` and `npm run typecheck` green; the workspace's Rust tests are
   untouched by this change and stay green.
6. The checklist below is run in a real session and its results are written into the PR body,
   one line per item, naming the reader version and the capture mode.

## Manual checklist (macOS, VoiceOver)

Driven at a live far end with the far end holding the line, since that is the only state the
defect lives in. Run 2026-09-03 against a `bash` over SSH to the `docker/ssh` rig, VoiceOver
15.0 on macOS 15.0, silent capture, `user` persona, with `ech` standing on the line —
agent-observed through the screen-readers bridge, every item.

- [x] `Cmd+K` opens the Connect dialog, and the far end's command line is unchanged. Said
      "Connect…" and landed in the connect list; `Escape` came back to a line still reading
      `ech`.
- [x] `Cmd+C` with text on the command line copies rather than typing a `c`. Said "Copy", and
      the line still read `ech`.
- [x] `Cmd+/` opens the help topic, and the far end's command line is unchanged. Said "Acter
      Help" on a level-2 heading inside the dialog; the line still read `ech` afterwards.
- [x] Typing and pressing Enter still runs a command, so nothing here cost the far end an
      ordinary key. `Tab` completed `ech` to `echo ` and said "o selecionado", `hi` typed onto
      the end, and Enter was answered by the far end's "hi" and a redrawn prompt.
- [x] `Ctrl+Shift+K` still hands the keyboard back, and says so: "Acter process keys."

Typing itself is silent on this platform, which is roadmap 36 rather than a finding of this
checklist: the row was read back with `VO-F4` at each step to confirm what it held.
