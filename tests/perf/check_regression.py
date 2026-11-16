"""Compare a metric workload result with a stored baseline.

Usage: check_regression.py <result file> <baseline file> <threshold percent> [<upper threshold percent>] [--ignore <key>[,<key>...]]

The optional upper threshold applies to the latency, memory and CPU keys.
Without it, the threshold applies to them too.
An ignored key is read and validated but not compared.

Exit codes: 0 pass, 1 regression, 2 bad input.
"""

import json
import math
import sys

HIGHER_IS_BETTER = {"samples_per_second"}
LOWER_IS_BETTER = {
    "query_p50_ms",
    "query_p95_ms",
    "query_p99_ms",
    "peak_rss_kib",
    "cpu_seconds",
}
KEYS = ["samples_per_second", *sorted(LOWER_IS_BETTER)]


class InputError(Exception):
    pass


def _number(value, where):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise InputError(f"{where} is not a number: {value!r}")
    if not math.isfinite(value):
        raise InputError(f"{where} is not finite: {value!r}")
    return float(value)


def check(result, baseline, threshold, upper_threshold=None, ignore=()):
    """Return one message per regression. An empty list means pass.

    `threshold` applies to throughput. `upper_threshold` applies to the
    latency, memory and CPU keys; when it is None it equals `threshold`.
    A key in `ignore` is validated like any other key but not compared.
    """
    if upper_threshold is None:
        upper_threshold = threshold
    ignored = set(ignore)
    unknown = sorted(ignored - set(KEYS))
    if unknown:
        raise InputError(f"ignore key is not a compared key: {unknown[0]}")
    t = threshold / 100.0
    u = upper_threshold / 100.0
    messages = []
    for key in KEYS:
        if key not in result:
            raise InputError(f"result has no key {key}")
        if key not in baseline:
            raise InputError(f"baseline has no key {key}")
        r = _number(result[key], f"result {key}")
        b = _number(baseline[key], f"baseline {key}")
        if b <= 0:
            raise InputError(f"baseline {key} must be > 0: {b}")
        if key in ignored:
            continue
        if key in HIGHER_IS_BETTER:
            limit = b * (1 - t)
            if r < limit:
                messages.append(
                    f"regression: {key} result={r} baseline={b} limit={limit}"
                )
        else:
            limit = b * (1 + u)
            if r > limit:
                messages.append(
                    f"regression: {key} result={r} baseline={b} limit={limit}"
                )
    return messages


def _read_result(path):
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                return json.loads(line)
    raise InputError(f"{path} has no JSON line")


def _read_baseline(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def _threshold(text):
    try:
        value = float(text)
    except ValueError:
        raise InputError(f"threshold is not a number: {text!r}")
    if not math.isfinite(value) or value < 0:
        raise InputError(f"threshold must be a number >= 0: {text!r}")
    return value


USAGE = (
    "usage: check_regression.py <result file> <baseline file> <threshold percent> "
    "[<upper threshold percent>] [--ignore <key>[,<key>...]]"
)


def main(argv):
    argv = list(argv)
    ignore = []
    if "--ignore" in argv:
        at = argv.index("--ignore")
        if at + 1 >= len(argv):
            print(USAGE, file=sys.stderr)
            return 2
        ignore = [name for name in argv[at + 1].split(",") if name]
        del argv[at : at + 2]
    if len(argv) not in (3, 4):
        print(USAGE, file=sys.stderr)
        return 2
    try:
        threshold = _threshold(argv[2])
        upper_threshold = _threshold(argv[3]) if len(argv) == 4 else None
        result = _read_result(argv[0])
        baseline = _read_baseline(argv[1])
        messages = check(result, baseline, threshold, upper_threshold, ignore)
    except (OSError, json.JSONDecodeError, InputError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    except KeyError as error:
        print(f"error: missing key {error}", file=sys.stderr)
        return 2
    if messages:
        for message in messages:
            print(message, file=sys.stderr)
        return 1
    print("perf ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
