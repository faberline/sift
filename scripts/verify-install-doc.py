#!/usr/bin/env python3
"""Check that every sh block of the install page says how its commands are tested.

Each block needs one marker as its nearest non-blank line before it:
`<!-- tested: <path> -->` (an existing file whose basename ci.yml runs) or
`<!-- manual: <reason> -->` (a reason of at least 10 characters).
Standard library only.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/install.md"
REQUIRED_HEADINGS = ("## install.sh", "## Container image", "## Docker Compose", "## Kubernetes")
TESTED = re.compile(r"^<!-- tested: (\S+) -->$")
MANUAL = re.compile(r"^<!-- manual: (.+) -->$")


def sh_blocks(page: str) -> list[tuple[int, str | None]]:
    """Return (line number, nearest non-blank line before it) for each sh block."""
    lines = page.splitlines()
    blocks = []
    for index, line in enumerate(lines):
        if line != "```sh":
            continue
        marker = None
        for before in reversed(lines[:index]):
            if before.strip():
                marker = before
                break
        blocks.append((index + 1, marker))
    return blocks


def problems(page: str, root: Path, ci_text: str) -> list[str]:
    """Return one message per problem. An empty list means the page passes."""
    found = []
    lines = page.splitlines()
    for heading in REQUIRED_HEADINGS:
        if heading not in lines:
            found.append(f"missing heading {heading}")
    blocks = sh_blocks(page)
    if not blocks:
        found.append("no sh block")
    for number, marker in blocks:
        tested = TESTED.match(marker) if marker is not None else None
        manual = MANUAL.match(marker) if marker is not None else None
        if tested is None and manual is None:
            found.append(f"line {number}: sh block has no tested or manual marker")
        elif manual is not None:
            if len(manual.group(1).strip()) < 10:
                found.append(f"line {number}: manual reason too short")
        else:
            path = tested.group(1)
            if not (root / path).is_file():
                found.append(f"line {number}: tested file {path} does not exist")
            elif Path(path).name not in ci_text:
                found.append(f"line {number}: tested file {path} is not run by ci.yml")
    return found


def main() -> int:
    page = (ROOT / PAGE).read_text(encoding="utf-8")
    ci_text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    found = problems(page, ROOT, ci_text)
    for message in found:
        print(message, file=sys.stderr)
    if found:
        return 1
    print(f"install doc ok: {len(sh_blocks(page))} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
