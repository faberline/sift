#!/usr/bin/env python3
"""Check that docs/operations/limits.md names only real files and items.

Each row of the `## Limits` table names the file and items where the code
enforces a limit. A row fails when its file does not exist or an item does
not occur verbatim in that file.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/limits.md"
MIN_ROWS = 30


def _is_separator(cells: list[str]) -> bool:
    return all(cell and set(cell) <= set("-:") for cell in cells)


def limit_rows(page: str) -> list[tuple[str, list[str]]]:
    lines = page.splitlines()
    try:
        start = lines.index("## Limits")
    except ValueError:
        raise ValueError("no Limits table") from None

    rows: list[tuple[str, list[str]]] = []
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if cells[0] == "Limit" or _is_separator(cells):
            continue
        spans = re.findall(r"`([^`]+)`", cells[2]) if len(cells) > 2 else []
        rows.append((cells[0], spans))

    if not rows:
        raise ValueError("no Limits table")
    return rows


def problems(rows: list[tuple[str, list[str]]], root: Path) -> list[str]:
    found: list[str] = []
    for limit, spans in rows:
        if not spans:
            found.append(f"{limit}: no enforcement location")
            continue
        path, items = spans[0], spans[1:]
        source = root / path
        if not source.is_file():
            found.append(f"{limit}: file {path} does not exist")
            continue
        text = source.read_text(encoding="utf-8")
        for item in items:
            if item not in text:
                found.append(f"{limit}: item {item} not found in {path}")
    return found


def main() -> int:
    page = (ROOT / PAGE).read_text(encoding="utf-8")
    rows = limit_rows(page)
    issues = problems(rows, ROOT)
    if "## Known limits" not in page.splitlines():
        issues.append("missing ## Known limits")
    if len(rows) < MIN_ROWS:
        issues.append(f"fewer than {MIN_ROWS} limits")
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        return 1
    print(f"limits doc ok: {len(rows)} limits")
    return 0


if __name__ == "__main__":
    sys.exit(main())
