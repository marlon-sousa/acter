# V1 — to a sighted person, it looks like a terminal

Lane 5, the look. It is the first of three PRs. V1 dresses the window as a terminal with
CSS. V2 puts the prompt, the command and the edit field on one visual line. V3 brings the far
end's colours into the output. This PR is V1 only.

## What is true today

[styles.css](../../ui/src/styles.css) holds readable defaults and no visual design, which
was deliberate from A1 onwards. A sighted person sees an unstyled web page: white
background, the system sans-serif font, thin black rules around the menu bar and the
status bar, grey default buttons, and dialogs as plain boxes. Each command's line is an
`h2` rendered as a large bold heading, and the output lines are small monospace text. The
visible labels "Command input" and "Command line" sit beside the edit fields. Nothing about
it says "terminal", and Acter is also a portfolio project that sighted people will judge
from a screenshot.

The page structure is already a terminal's. The buffer is a prompt paragraph, then the
command's heading, then its output rows, then the next prompt, top to bottom, and the edit
field is below the buffer. Only the presentation differs.

## Decisions

### 1. CSS only. The markup does not change

Every element, role, name, label, `hidden` attribute and element order stays as it is. No
file under `ui/src/views/` or `ui/src/adapters/` changes. What NVDA reads comes from the
page structure, which is why that is the line this PR does not cross. The one change
outside CSS is the window's `theme` in `tauri.conf.json` (decision 5).

A packaged design system (Fluent UI Web Components, Material, Bootstrap) was considered and
is not used. It would replace the menu bar, the option lists, the dialogs and the far-end
field, which were built by hand and measured against NVDA, with its own widgets.

### 2. The model is Windows Terminal with its default settings

It is the terminal a Windows user already knows, and its defaults are its own:

- The session area uses Cascadia Mono, falling back to Consolas and then `monospace`.
  Cascadia Mono ships with Windows 11 and with Windows Terminal.
- The Campbell scheme's background, `#0C0C0C`, and foreground, `#CCCCCC`.
- No boxes, rules or gaps between commands. Output reads as one continuous run of lines.

The buffer shows no colour from the far end. V3 adds that, and the palette is chosen there.

### 3. Headings look like terminal lines and stay headings

The command's `h2` takes the output's font, size and line height, with no bold and no margin,
so on screen it is just the line the user typed. It gets one visual difference: it is drawn
in full white, `#F2F2F2`, against the output's `#CCCCCC`. That lets a sighted helper still
find where each command starts. No text is added with `::before` or `::after`, because
generated content is read aloud.

The prompt paragraph gets the same treatment, with no margin.

The window's own `h1`, which repeats the window title (spec A9), is today the largest thing on
screen and can run to three lines with a long connection name. It becomes a slim strip above
the session, like a Windows Terminal tab: the interface font at a small size, muted, on one
line, cut short with an ellipsis when it does not fit. The ellipsis is visual only; the
heading's text is unchanged, so NVDA still reads it in full.

### 4. The edit fields look like the terminal's own line

- `#command-input` and `#far-end-input` get no border, no background of their own, the
  session font, and the session colours. The `input` gets a white caret.
- Their labels, "Command input" and "Command line", are hidden visually with CSS and stay in
  the accessibility tree. The rules are the same as the existing `.visually-hidden` class,
  applied by selector, so the markup still does not change.
- The "Not connected." state and its Connect button use the session font and colours. The
  button is outlined, not filled.

### 5. The whole window is dark, and the title bar matches

Windows Terminal is dark by default whatever the Windows theme is, and a terminal with a
white menu bar above a black session looks unfinished. The menu bar, its menus, the status
bar and the dialogs therefore get a dark surface: `#1F1F1F` for bars and menus and
`#2B2B2B` for dialogs. Their text is Segoe UI Variable, falling back to `system-ui`, because
Windows Terminal's own chrome uses it.

`tauri.conf.json` sets the window's `theme` to `"Dark"`, so the native title bar is dark too.

A light variant is not in this PR. Low-vision users who need a different contrast get it from
decision 6. A light theme can be an entry of its own if anyone asks for one.

### 6. Windows high contrast wins over every colour here

All colour rules sit where `@media (forced-colors: active)` can override them. Under a Windows
contrast theme the page uses system colours (`Canvas`, `CanvasText`, `Highlight`,
`HighlightText`, `ButtonText`) and draws borders on the edit fields and dialogs again,
because without our colours they are the only thing that shows where a field is. The Connect
dialog's selected option keeps `Highlight` and `HighlightText` in both modes, as it does
today.

### 7. Focus is always visible

Every focusable element gets a 2px outline in a colour that passes 3:1 against its surface.
That includes the focused heading when the buffer takes focus, the menu items, the buttons
and the option lists. On the edit fields the caret is the focus indicator, as it is in a
terminal, and the field itself gets no outline.

### 8. Size and zoom still work

Sizes are in `rem`, so the window follows WebView2's zoom and the user's text-size setting.
Nothing is clipped or overlapped at 200% zoom in a 900 by 600 window, apart from the long
output lines, which wrap as they do today.

## What V1 does not do

- **Putting the prompt, the command and the edit field on one line.** That is V2. It needs
  its own measurement. NVDA's browse mode decides what counts as one line partly from the
  page layout (its "use screen layout" setting), so two elements drawn on one visual line
  may be read as one line. V2 also has to find out whether, in far-end line mode, the far
  end's row already includes the prompt text, which would show it twice.
- **Colour in the output.** That is V3, and it changes the protocol, not CSS.
- **Full-screen programs.** The grid renderer stays behind the phase 2 gate. `nano` still
  gets Acter's "needs interactive mode" message.
- **Mica or any other transparency.** A terminal's background is solid for legibility.

## Files touched

- `ui/src/styles.css`
- `crates/acter-app/tauri.conf.json` (the window's `theme`)
- `docs/ROADMAP.md` (flip V1 to Done)

## Acceptance criteria

1. No file under `ui/src/views/` or `ui/src/adapters/` changes.
2. `npm run test:ui`, `npm run typecheck` and `npm run test:e2e` pass with no test edited.
   That includes the axe spec, whose default rules include colour contrast.
3. Contrast, measured and written in the PR body: output text against the background at 4.5:1
   or more, and command text, bar text and dialog text at 4.5:1 or more against their
   surfaces. Focus outlines at 3:1 or more.
4. Screenshots of the app, taken by the agent through the e2e WebDriver and described in
   words in the PR body, in these states: not connected; a session with three commands; the
   Connect dialog; a menu open; far-end line mode; the same session under a Windows contrast
   theme; and the same session at 200% zoom.

## Manual accessibility checklist (PR body)

- NVDA browse mode reads a three-command buffer heading by heading and line by line exactly as
  it did on main. The speech is captured through the bridge on both builds and compared.
- The edit field and the far-end field are announced by their names ("Command input", "Command
  line") as before, although the labels are no longer visible.
- A sighted person looks at the screenshots and says what kind of program it is. This item is
  human-only.
