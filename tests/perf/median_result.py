"""Take the median of several trace workload results.

Usage: median_result.py <output file> <result file> <result file> ...

Each result file holds one JSON line (the first non-empty line is read). The
output is one JSON line: a copy of the first result, with each compared key
replaced by its median over the runs, plus `runs` and `cpu_seconds_runs`.
The `cpu_seconds` of each run is also printed, one line per run, then the
median. The number of results must be odd and at least 3.

Exit codes: 0 ok, 2 bad input.
"""

import json
import math
import statistics
import sys

KEYS = ["samples_per_second", "query_p50_ms", "query_p95_ms", "query_p99_ms", "peak_rss_kib", "cpu_seconds"]

USAGE = "usage: median_result.py <output file> <result file> <result file> ..."


class InputError(Exception):
    pass


def median_result(results):
    if len(results) % 2 == 0 or len(results) < 3:
        raise InputError(f"need an odd number of results, at least 3: {len(results)}")
    for i, run in enumerate(results, start=1):
        for key in KEYS:
            if key not in run:
                raise InputError(f"result {i} has no key {key}")
            value = run[key]
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
                raise InputError(f"result {i} {key} is not a number: {value!r}")
    median = dict(results[0])
    for key in KEYS:
        median[key] = statistics.median([run[key] for run in results])
    median["runs"] = len(results)
    median["cpu_seconds_runs"] = [run["cpu_seconds"] for run in results]
    return median


def _read_result(path):
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                return json.loads(line)
    raise InputError(f"{path} has no JSON line")


def main(argv):
    argv = list(argv)
    if len(argv) < 4:
        print(USAGE, file=sys.stderr)
        return 2
    output, paths = argv[0], argv[1:]
    try:
        results = [_read_result(path) for path in paths]
        median = median_result(results)
        with open(output, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(median) + "\n")
    except (OSError, json.JSONDecodeError, InputError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    for i, run in enumerate(results, start=1):
        print(f"run {i} cpu_seconds={run['cpu_seconds']}")
    print(f"median cpu_seconds={median['cpu_seconds']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
