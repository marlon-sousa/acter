# V2 — the prompt, the command and the edit field share one visual line

Lane 5, the look; roadmap entry 53. It is the second of three PRs: V1 dressed the window as a
terminal, V2 folds the prompt onto the line it belongs to, and V3 will bring the far end's
colours into the output.

## What is true today

After V1, the buffer is one run of terminal lines, but a real terminal would put some of them
together. The buffer holds each prompt as a `p.prompt`, then the command as an `h2`, then its
output rows, as siblings, and the edit field is a separate form after `#results`. So on
screen the prompt and the command sit on separate lines, and the cursor sits on a line of its
own below the last prompt.

The entry left two questions open. NVDA's browse mode decides what counts as one line partly
from the page layout, so two elements drawn on one visual line might be read as one. And in
far-end line mode, the far end's row might already include the prompt text, which would show
it twice.

## What was measured

**On 2026-09-23, against a real PowerShell** (`ACTER_SHELL=powershell.exe`), the prototype
driven over the embedded WebDriver:

- **The buffer's order is prompt, heading, output, prompt, heading, output, prompt.** A
  prompt comes before the command it introduces, and a prompt ends the buffer while the
  shell waits.
- **In far-end line mode the far end's field holds only what is typed.** With `echo hi` typed,
  `#far-end-input` read `echo hi`, and the buffer's last child was the prompt
  `PS C:\Users\marlo>`. Folded, the screen shows `PS C:\Users\marlo> echo hi` once.

**NVDA 2026.1.1 through the screen-readers bridge**, `user` persona, silent capture, the same
PowerShell session. Steps: `echo hello`, Enter, `Get-Date`, Enter; then, in browse mode,
Ctrl+Home, twelve Down arrows, Ctrl+Home, `h` three times, `e`, and two Up arrows. It was
read twice on the same page: first with the fold, then with the fold switched off through
WebDriver (`float: none`), which is V1's layout. Both readings were identical, line for line:
the menu bar; the level 1 title; "Results region" and the PSReadLine warning over two lines;
`PS C:\Users\marlo>`; "heading level 2, echo hello"; `hello`; the prompt; "heading level 2,
Get-Date"; the date; the prompt; "out of region, section, Command input"; "edit". Heading
navigation gave the same three stops, and `e` landed on the Command input in both.

**Against the scripted fake**, a prompt is drawn only when the next command is submitted,
which is entry 52.1. The fold still joins each prompt to the command after it, but no prompt
waits at the end, so the edit field has nothing to join.

## Decisions

1. **CSS only, with floats.** A prompt followed directly by a heading, and the buffer's last
   prompt, float left with one character of space after them. The heading's text, or the edit
   field, then starts beside the prompt. A float keeps the element `display: block`, which is
   what NVDA reads (see above). Nothing under `ui/src/views/` or `ui/src/adapters/` changes.
2. **The edit field needs no rule of its own.** `#command-form` and `#far-end-line` are flex
   containers, which sit beside a float, so the last prompt's float, which leaves `#results`,
   puts either field on its line.
3. **A prompt followed by anything other than a heading stays on its own line.** Two prompts in
   a row, which is what pressing Enter on an empty line gives, show as a terminal shows them:
   the first alone and the second with what follows it.
4. **Output and the ended state clear the float,** so a short prompt never pulls the first
   output row up beside the command.

## What V2 does not do

- It does not change where the app puts prompts. That is entry 52.1.
- Colour is V3.

## Files touched

- `ui/src/styles.css`
- `docs/ROADMAP.md` (flip 53 to Done)

## Acceptance criteria

1. No file under `ui/src/views/` or `ui/src/adapters/` changes. `npm run test:ui`,
   `npm run typecheck` and `npm run test:e2e` pass with no test edited.
2. Against PowerShell, a screenshot shows each prompt on the same line as its command, and the
   last prompt on the same line as the edit field, in both line modes.
3. NVDA reads the buffer the same with and without the fold, as measured above.
