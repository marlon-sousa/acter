---
name: accessibility-checklist
description: How to run a manual accessibility checklist item for Acter through the screen-readers MCP bridge, which items an agent may check and which stay human-only, and which bridge persona to drive with. Use before touching the bridge for any checklist, PR body checkbox, or NVDA or VoiceOver observation.
---

# Running a checklist item

Checklist results live in the implementing PR's body as checkboxes, one item per
check, with findings written inline on the unchecked item: reader version, expected,
observed. There is no separate findings document. A finding that needs a change becomes
a roadmap entry.

## Who may check an item

An agent may run and record any item it can actually observe: drive a real screen
reader through the screen-readers bridge and record what was spoken. Such an item is
checked by the agent, and its observation is written inline exactly as a human's would
be, naming the reader version and the capture mode.

Items an agent cannot observe stay the human's to verify and check. The bridge captures
speech and braille, not audio, so anything that turns on a beep, a sound cue, or
subjective comfort is human-only.

State plainly in the PR body which items were agent-observed and which were
human-verified. A checked box must never imply a sense nobody used.

## Mode surprises

The tester's NVDA does not switch focus mode automatically. Read the current mode and set
the one the item needs rather than assuming it, and never report a mode artifact as a
defect in the software under test.

## Which persona to connect as

Connect to the bridge as `user`, not `validator`. Acter is for screen reader users, so a
checklist item asks whether an ordinary, non-expert user can hear and do the thing. That
is only answered by driving the way one drives: focus, Tab, the arrows, typing, and the
reader's ordinary reading commands. A stance with more reach answers a different
question, and an item checked from it would claim reachability the product has not
earned.

`validator` has one purpose, stated when used: characterising a UI failure already
found, where introspection says what is wrong rather than only that something is. Never
use it to get past a failure.

`expert` is for working out how the reader itself behaves, not for judging Acter.

## Recording an observation

Write what was spoken, verbatim where it matters, with the reader and version, the
capture mode, and the keystrokes that produced it. One item, one observation. If the
observation contradicts the item's expectation, leave the box unchecked and write both.
