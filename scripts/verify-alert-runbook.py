#!/usr/bin/env python3
"""Check that every alert kind that Sift ships has a complete runbook entry.

The alert kinds are the variants of `RuleSource` in src/alerting/domain/rule.rs.
Each kind needs a `### <kind>` entry in the `## Alerts` section of
docs/operations/runbook.md, with the four labels and a first command.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LABELS = ("**Meaning**", "**First command**", "**Next step**", "**Verified**")
VARIANT_RE = re.compile(r"^    ([A-Z][A-Za-z0-9]*)(\s*\{.*|,|\s*)$")


def _camel_to_snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def shipped_alerts(rule_source):
    """Return the snake_case names of the RuleSource variants, in file order."""
    start = rule_source.find("pub enum RuleSource {")
    if start < 0:
        raise ValueError("RuleSource not found")
    body_start = rule_source.index("{", start) + 1
    end = rule_source.find("\n}", body_start)
    body = rule_source[body_start:end if end >= 0 else len(rule_source)]
    kinds = []
    depth = 0
    for line in body.splitlines():
        if depth == 0:
            m = VARIANT_RE.match(line)
            if m:
                kinds.append(_camel_to_snake(m.group(1)))
        depth += line.count("{") - line.count("}")
    if not kinds:
        raise ValueError("RuleSource not found")
    return kinds


def runbook_entries(runbook):
    """Return {heading: body} for the `### ` entries of the `## Alerts` section."""
    lines = runbook.splitlines()
    try:
        first = lines.index("## Alerts")
    except ValueError:
        return {}
    section = []
    for line in lines[first + 1:]:
        if line.startswith("## "):
            break
        section.append(line)
    entries = {}
    heading = None
    body = []
    for line in section:
        if line.startswith("### "):
            if heading is not None:
                entries[heading] = "\n".join(body)
            heading = line[4:].strip()
            body = []
        elif heading is not None:
            body.append(line)
    if heading is not None:
        entries[heading] = "\n".join(body)
    return entries


def problems(kinds, entries):
    """Return one message per missing entry, label or code block."""
    found = []
    for kind in kinds:
        if kind not in entries:
            found.append(f"alert {kind}: no runbook entry")
            continue
        body = entries[kind]
        for label in LABELS:
            if label not in body:
                found.append(f"alert {kind}: missing {label}")
        if "**First command**" in body and not any(
            line.startswith("```") for line in body.splitlines()
        ):
            found.append(f"alert {kind}: first command has no code block")
    return found


def main():
    rule = (ROOT / "src/alerting/domain/rule.rs").read_text(encoding="utf-8")
    runbook = (ROOT / "docs/operations/runbook.md").read_text(encoding="utf-8")
    kinds = shipped_alerts(rule)
    found = problems(kinds, runbook_entries(runbook))
    if found:
        for message in found:
            print(message, file=sys.stderr)
        return 1
    print(f"alert runbook ok: {', '.join(kinds)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
