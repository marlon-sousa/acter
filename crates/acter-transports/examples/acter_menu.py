#!/usr/bin/env python3
"""A gh-shaped inline selection prompt, for the far-end-line checklist.

It draws the way `gh` does and for the same reasons: options printed below the
question, the cursor hidden for the whole prompt, and the option rows rewritten
in place on every arrow. No alternate screen -- that is a different path in
Acter -- and it creates nothing, which is why it stands in for `gh repo create`
in checklist items 6 and 7.
"""
import sys, termios, tty

OPTIONS = [
    "Create a new repository from scratch",
    "Create a new repository from a template",
    "Push an existing local repository",
]

def draw(chosen, first):
    if not first:
        sys.stdout.write("\x1b[%dA" % len(OPTIONS))
    for index, option in enumerate(OPTIONS):
        marker = ">" if index == chosen else " "
        sys.stdout.write("\r\x1b[K%s %s\n" % (marker, option))
    sys.stdout.flush()

def main():
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    chosen = 0
    print("? What would you like to do? [Use arrows to move]")
    sys.stdout.write("\x1b[?25l")
    draw(chosen, True)
    try:
        tty.setraw(fd)
        while True:
            key = sys.stdin.read(1)
            if key == "\r" or key == "\n":
                break
            if key == "\x03":
                chosen = None
                break
            if key == "\x1b":
                if sys.stdin.read(1) == "[":
                    arrow = sys.stdin.read(1)
                    if arrow == "A":
                        chosen = (chosen - 1) % len(OPTIONS)
                    elif arrow == "B":
                        chosen = (chosen + 1) % len(OPTIONS)
                    else:
                        continue
                    draw(chosen, False)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)
        sys.stdout.write("\x1b[?25h")
        sys.stdout.flush()
    if chosen is None:
        print("\nnothing chosen")
    else:
        print("\nchose: %s" % OPTIONS[chosen])

main()
