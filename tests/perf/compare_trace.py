"""Compare the head trace median with the base trace median measured in the same job.

Usage: compare_trace.py <head median file> <base median file> <threshold percent>

Both files come from median_result.py. The rule is the one of check_regression.py,
with the base commit as the reference: head may be at most 160 percent of base
for latency, memory and CPU, and throughput may not fall below 40 percent of base.
A job summary is written when GITHUB_STEP_SUMMARY is set.

Exit codes: 0 pass, 1 regression, 2 bad input.
"""

import json
import os
import sys

import check_regression

USAGE = "usage: compare_trace.py <head median file> <base median file> <threshold percent>"


def comparison_rows(head, base):
    """One row (key, base value, head value, head/base ratio) for each key in check_regression.KEYS."""
    rows = []
    for key in check_regression.KEYS:
        if key not in head:
            raise check_regression.InputError(f"head has no key {key}")
        if key not in base:
            raise check_regression.InputError(f"base has no key {key}")
        head_value = check_regression._number(head[key], f"head {key}")
        base_value = check_regression._number(base[key], f"base {key}")
        if base_value <= 0:
            raise check_regression.InputError(f"base {key} must be > 0: {base_value}")
        rows.append((key, base_value, head_value, head_value / base_value))
    return rows


def summary_markdown(rows, threshold, messages):
    lines = [
        "| key | base median | head median | ratio |",
        "| --- | --- | --- | --- |",
    ]
    for key, base_value, head_value, ratio in rows:
        lines.append(f"| {key} | {base_value} | {head_value} | {ratio:.3f} |")
    lines.append("")
    lines.append(f"threshold: {threshold:g} percent against the base commit")
    lines.append("")
    lines.append("result: regression" if messages else "result: pass")
    for message in messages:
        lines.append(f"- {message}")
    return "\n".join(lines)


def _write_summary(text):
    path = os.environ.get("GITHUB_STEP_SUMMARY", "")
    if not path:
        return
    try:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(text + "\n")
    except OSError as error:
        print(f"warning: cannot write the job summary: {error}", file=sys.stderr)


def main(argv):
    argv = list(argv)
    if len(argv) != 3:
        print(USAGE, file=sys.stderr)
        return 2
    try:
        threshold = check_regression._threshold(argv[2])
        head = check_regression._read_result(argv[0])
        base = check_regression._read_result(argv[1])
        messages = check_regression.check(head, base, threshold)
        rows = comparison_rows(head, base)
    except (OSError, ValueError, check_regression.InputError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(f"trace regression check: head median against base median, threshold {threshold:g}%")
    for key, base_value, head_value, ratio in rows:
        print(f"{key} base={base_value} head={head_value} ratio={ratio:.3f}")
    _write_summary(summary_markdown(rows, threshold, messages))
    if messages:
        for message in messages:
            print(message, file=sys.stderr)
        return 1
    print("perf ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
