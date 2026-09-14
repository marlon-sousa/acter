# 35 — M4, bundling, signing and notarising Acter itself.

Lane: lane-3-macos. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**M4, bundling, signing and notarising Acter itself.** Spec: none yet → specify first.
`bundle.active` is `false` and the identifier is `dev.marlonsousa.acter`. Distribution
is outside the App Store, so this is Developer ID signing plus notarisation, and it is
last deliberately: it is about handing Acter to somebody else, not about Acter working.

**It also fixes two things M3 measured and refused to work around.** The macOS
application menu and its Quit, Hide and About items are named by the *process*, so today
a listener hears "acter-app" where the product is called Acter; a bundle is what gives
the process the product's name, in every language macOS translates those items into. And
33.1 — VoiceOver told that nothing has keyboard focus — was measured on an unbundled
binary, which is a confound this entry removes before that one is judged.
