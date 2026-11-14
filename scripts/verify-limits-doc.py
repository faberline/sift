#!/usr/bin/env python3
"""Check that docs/operations/limits.md names only real files and items.

Each row of the `## Limits` table names a source file and the items where the
code enforces the limit. This check fails when a named file is missing or a
named item does not occur in that file.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/limits.md"
MIN_ROWS = 30


def limit_rows(page: str) -> list[tuple[str, list[str]]]:
    """Return (limit name, backticked spans of the third cell) for each row."""
    lines = page.splitlines()
    try:
        start = lines.index("## Limits")
    except ValueError:
        raise ValueError("no Limits table") from None
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("## "):
            end = index
            break
    rows: list[tuple[str, list[str]]] = []
    for line in lines[start + 1 : end]:
        if not line.startswith("|"):
            continue
        cells = line.strip().strip("|").split("|")
        name = cells[0].strip()
        if name == "Limit" or set(line) <= set("|-: "):
            continue
        spans = re.findall(r"`([^`]+)`", cells[2]) if len(cells) > 2 else []
        rows.append((name, spans))
    if not rows:
        raise ValueError("no Limits table")
    return rows


def problems(rows: list[tuple[str, list[str]]], root: Path) -> list[str]:
    """Return one message for each named location that does not exist."""
    found: list[str] = []
    for name, spans in rows:
        if not spans:
            found.append(f"{name}: no enforcement location")
            continue
        path = spans[0]
        source = root / path
        if not source.is_file():
            found.append(f"{name}: file {path} does not exist")
            continue
        text = source.read_text(encoding="utf-8", errors="replace")
        for item in spans[1:]:
            if item not in text:
                found.append(f"{name}: item {item} not found in {path}")
    return found


def main() -> int:
    page = (ROOT / PAGE).read_text(encoding="utf-8")
    errors: list[str] = []
    if "## Known limits" not in page.splitlines():
        errors.append("missing ## Known limits")
    rows = limit_rows(page)
    if len(rows) < MIN_ROWS:
        errors.append(f"fewer than {MIN_ROWS} limits")
    errors.extend(problems(rows, ROOT))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"limits doc ok: {len(rows)} limits")
    return 0


if __name__ == "__main__":
    sys.exit(main())
