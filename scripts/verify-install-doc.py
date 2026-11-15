#!/usr/bin/env python3
"""Check that every sh block of docs/operations/install.md has a test marker.

A block is either `<!-- tested: <path> -->`, where the named repository file
runs its commands and its basename occurs in ci.yml, or
`<!-- manual: <reason> -->`, where the reason says why no test runs it.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/install.md"
REQUIRED_HEADINGS = ("## install.sh", "## Container image", "## Docker Compose", "## Kubernetes")
TESTED = re.compile(r"^<!-- tested: (\S+) -->$")
MANUAL = re.compile(r"^<!-- manual: (.+) -->$")
MIN_REASON = 10


def sh_blocks(page: str) -> list[tuple[int, str | None]]:
    lines = page.splitlines()
    blocks: list[tuple[int, str | None]] = []
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
    found: list[str] = []
    headings = page.splitlines()
    for heading in REQUIRED_HEADINGS:
        if heading not in headings:
            found.append(f"missing heading {heading}")

    blocks = sh_blocks(page)
    if not blocks:
        found.append("no sh block")

    for number, marker in blocks:
        tested = TESTED.match(marker) if marker else None
        manual = MANUAL.match(marker) if marker else None
        if tested:
            path = tested.group(1)
            source = root / path
            if not source.is_file():
                found.append(f"line {number}: tested file {path} does not exist")
            elif source.name not in ci_text:
                found.append(f"line {number}: tested file {path} is not run by ci.yml")
        elif manual:
            if len(manual.group(1).strip()) < MIN_REASON:
                found.append(f"line {number}: manual reason too short")
        else:
            found.append(f"line {number}: sh block has no tested or manual marker")
    return found


def main() -> int:
    page = (ROOT / PAGE).read_text(encoding="utf-8")
    ci_text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    issues = problems(page, ROOT, ci_text)
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        return 1
    print(f"install doc ok: {len(sh_blocks(page))} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
