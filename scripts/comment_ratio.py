"""Count comment lines against code lines in Rust and TypeScript files.

Usage: python scripts/comment_ratio.py [path ...]
With no arguments, every tracked .rs and .ts file except ui/src/protocol.ts.
Prints one line per file and a total: comment lines, code lines, and the ratio.
"""
import subprocess
import sys


def count(path: str) -> tuple[int, int]:
    comment = code = 0
    in_block = False
    with open(path, encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            line = raw.strip()
            if in_block:
                comment += 1
                if "*/" in line:
                    in_block = False
            elif not line:
                continue
            elif line.startswith("//"):
                comment += 1
            elif line.startswith("/*"):
                comment += 1
                in_block = "*/" not in line
            else:
                code += 1
    return comment, code


def tracked() -> list[str]:
    files = subprocess.check_output(["git", "ls-files"], text=True).split()
    return [
        f for f in files
        if f.endswith((".rs", ".ts")) and "node_modules" not in f and not f.endswith("ui/src/protocol.ts")
    ]


def main(argv: list[str]) -> None:
    paths = argv or tracked()
    total_comment = total_code = 0
    for path in paths:
        comment, code = count(path)
        total_comment += comment
        total_code += code
        print(f"{path}: {comment} comment, {code} code, {comment / max(code, 1):.2f} per code line")
    print(f"TOTAL: {total_comment} comment, {total_code} code, {total_comment / max(total_code, 1):.2f} per code line")


if __name__ == "__main__":
    main(sys.argv[1:])
