import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import check_regression
import median_result

BASELINE = {
    "samples_per_second": 1000,
    "query_p50_ms": 10,
    "query_p95_ms": 20,
    "query_p99_ms": 30,
    "peak_rss_kib": 100000,
    "cpu_seconds": 10,
}

SLOW_RUN = {
    "samples_per_second": 2417.749,
    "query_p50_ms": 447.743,
    "query_p95_ms": 465.851,
    "query_p99_ms": 995.746,
    "peak_rss_kib": 298772,
    "cpu_seconds": 44.78,
}

SLOW_LOG_RUN = {
    "samples_per_second": 2837.648,
    "query_p50_ms": 10.677,
    "query_p95_ms": 24.982,
    "query_p99_ms": 272.324,
    "peak_rss_kib": 505784,
    "cpu_seconds": 27.65,
}

# Runs 1, 3 and 5 are measured on CI (43696743112, 43913197544, 43991449604).
# Runs 2 and 4 are test samples, not measurements.
TRACE_RUNS = [
    {"samples_written": 50000, "queried_samples": 21000, "samples_per_second": 4029.727, "query_p50_ms": 676.363, "query_p95_ms": 680.546, "query_p99_ms": 683.482, "peak_rss_kib": 466192, "cpu_seconds": 32.65, "requests": 50, "items_per_request": 1000, "queries": 21},
    {"samples_written": 50000, "queried_samples": 21000, "samples_per_second": 3900.0, "query_p50_ms": 690.0, "query_p95_ms": 695.0, "query_p99_ms": 700.0, "peak_rss_kib": 466000, "cpu_seconds": 34.0, "requests": 50, "items_per_request": 1000, "queries": 21},
    {"samples_written": 50000, "queried_samples": 21000, "samples_per_second": 2652.503, "query_p50_ms": 977.881, "query_p95_ms": 984.382, "query_p99_ms": 997.561, "peak_rss_kib": 465828, "cpu_seconds": 49.81, "requests": 50, "items_per_request": 1000, "queries": 21},
    {"samples_written": 50000, "queried_samples": 21000, "samples_per_second": 3800.0, "query_p50_ms": 700.0, "query_p95_ms": 705.0, "query_p99_ms": 710.0, "peak_rss_kib": 466100, "cpu_seconds": 35.0, "requests": 50, "items_per_request": 1000, "queries": 21},
    {"samples_written": 50000, "queried_samples": 21000, "samples_per_second": 2759.793, "query_p50_ms": 1035.765, "query_p95_ms": 1044.052, "query_p99_ms": 1045.317, "peak_rss_kib": 465700, "cpu_seconds": 53.4, "requests": 50, "items_per_request": 1000, "queries": 21},
]


def with_values(**changes):
    result = dict(BASELINE)
    result.update(changes)
    return result


class CheckRegression(unittest.TestCase):
    def test_passes_within_threshold(self):
        result = with_values(query_p95_ms=24, samples_per_second=850)
        self.assertEqual(check_regression.check(result, BASELINE, 20), [])

    def test_fails_when_latency_exceeds_threshold(self):
        # Without check_regression.py this module fails to import, so the test fails.
        result = with_values(query_p99_ms=37)
        messages = check_regression.check(result, BASELINE, 20)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])

    def test_fails_when_throughput_falls(self):
        result = with_values(samples_per_second=790)
        messages = check_regression.check(result, BASELINE, 20)
        self.assertEqual(len(messages), 1)
        self.assertIn("samples_per_second", messages[0])

    def test_fails_on_memory_and_cpu(self):
        result = with_values(peak_rss_kib=130000, cpu_seconds=13)
        messages = check_regression.check(result, BASELINE, 20)
        self.assertEqual(len(messages), 2)

    def test_exact_limit_passes(self):
        baseline = dict(BASELINE, query_p50_ms=10)
        result = with_values(query_p50_ms=15)
        self.assertEqual(check_regression.check(result, baseline, 50), [])

    def test_log_baseline_has_every_compared_key(self):
        path = Path(__file__).parent / "baseline" / "log.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        for key in (
            "samples_per_second",
            "query_p50_ms",
            "query_p95_ms",
            "query_p99_ms",
            "peak_rss_kib",
            "cpu_seconds",
        ):
            self.assertIn(key, baseline)
            self.assertIsInstance(baseline[key], (int, float))
            self.assertGreater(baseline[key], 0, key)

    def test_log_baseline_fails_when_exceeded_and_passes_unchanged(self):
        path = Path(__file__).parent / "baseline" / "log.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(check_regression.check(baseline, baseline, 60), [])

        bad = dict(baseline)
        bad["query_p95_ms"] = baseline["query_p95_ms"] * 1.7
        messages = check_regression.check(bad, baseline, 60)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p95_ms", messages[0])

    def test_trace_baseline_has_every_compared_key(self):
        path = Path(__file__).parent / "baseline" / "trace.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        for key in (
            "samples_per_second",
            "query_p50_ms",
            "query_p95_ms",
            "query_p99_ms",
            "peak_rss_kib",
            "cpu_seconds",
        ):
            self.assertIn(key, baseline)
            self.assertIsInstance(baseline[key], (int, float))
            self.assertGreater(baseline[key], 0, key)

    def test_trace_baseline_fails_when_exceeded_and_passes_unchanged(self):
        path = Path(__file__).parent / "baseline" / "trace.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(check_regression.check(baseline, baseline, 60), [])

        bad = dict(baseline)
        bad["query_p99_ms"] = baseline["query_p99_ms"] * 1.7
        messages = check_regression.check(bad, baseline, 60)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            baseline_path = tmp / "baseline.json"
            baseline_path.write_text(json.dumps(BASELINE))

            passing = tmp / "pass.json"
            passing.write_text(json.dumps(with_values(query_p95_ms=24)) + "\n")
            self.assertEqual(
                check_regression.main([str(passing), str(baseline_path), "20"]), 0
            )

            failing = tmp / "fail.json"
            failing.write_text(json.dumps(with_values(query_p99_ms=37)) + "\n")
            self.assertEqual(
                check_regression.main([str(failing), str(baseline_path), "20"]), 1
            )

            missing_key = tmp / "missing.json"
            partial = dict(BASELINE)
            del partial["query_p99_ms"]
            missing_key.write_text(json.dumps(partial) + "\n")
            self.assertEqual(
                check_regression.main([str(missing_key), str(baseline_path), "20"]), 2
            )

            self.assertEqual(
                check_regression.main(
                    [str(tmp / "no-such-file.json"), str(baseline_path), "20"]
                ),
                2,
            )

    def test_upper_threshold_applies_to_lower_is_better_keys(self):
        result = with_values(query_p50_ms=25)
        self.assertEqual(check_regression.check(result, BASELINE, 20, 150), [])
        messages = check_regression.check(result, BASELINE, 20)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p50_ms", messages[0])

    def test_upper_threshold_does_not_change_throughput(self):
        result = with_values(samples_per_second=790)
        messages = check_regression.check(result, BASELINE, 20, 150)
        self.assertEqual(len(messages), 1)
        self.assertIn("samples_per_second", messages[0])

    def test_metric_baseline_accepts_slow_runner(self):
        path = Path(__file__).parent / "baseline" / "metric.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(check_regression.check(SLOW_RUN, baseline, 60, 160), [])
        self.assertNotEqual(check_regression.check(SLOW_RUN, baseline, 60), [])
        self.assertEqual(check_regression.check(baseline, baseline, 60, 160), [])

    def test_metric_baseline_still_catches_large_regression(self):
        path = Path(__file__).parent / "baseline" / "metric.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))

        bad = dict(baseline, cpu_seconds=baseline["cpu_seconds"] * 3)
        messages = check_regression.check(bad, baseline, 60, 160)
        self.assertEqual(len(messages), 1)
        self.assertIn("cpu_seconds", messages[0])

        bad = dict(baseline, query_p99_ms=baseline["query_p99_ms"] * 3)
        messages = check_regression.check(bad, baseline, 60, 160)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])

    def test_main_upper_threshold_arguments(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            baseline_path = tmp / "baseline.json"
            baseline_path.write_text(json.dumps(BASELINE))

            result_path = tmp / "result.json"
            result_path.write_text(json.dumps(with_values(query_p50_ms=25)) + "\n")
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "150"]
                ),
                0,
            )
            self.assertEqual(
                check_regression.main([str(result_path), str(baseline_path), "20"]),
                1,
            )
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "x"]
                ),
                2,
            )
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "150", "1"]
                ),
                2,
            )

    def test_log_baseline_accepts_slow_runner(self):
        path = Path(__file__).parent / "baseline" / "log.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(
            check_regression.check(SLOW_LOG_RUN, baseline, 60, 150, ["query_p99_ms"]),
            [],
        )
        messages = check_regression.check(SLOW_LOG_RUN, baseline, 60, 150)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])
        self.assertNotEqual(check_regression.check(SLOW_LOG_RUN, baseline, 60), [])
        self.assertEqual(
            check_regression.check(baseline, baseline, 60, 150, ["query_p99_ms"]),
            [],
        )

    def test_log_baseline_still_catches_large_regression(self):
        path = Path(__file__).parent / "baseline" / "log.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))
        for key in ("query_p50_ms", "query_p95_ms", "cpu_seconds"):
            bad = dict(baseline, **{key: baseline[key] * 3})
            messages = check_regression.check(bad, baseline, 60, 150, ["query_p99_ms"])
            self.assertEqual(len(messages), 1, key)
            self.assertIn(key, messages[0])

        bad = dict(baseline, samples_per_second=baseline["samples_per_second"] / 3)
        messages = check_regression.check(bad, baseline, 60, 150, ["query_p99_ms"])
        self.assertEqual(len(messages), 1)
        self.assertIn("samples_per_second", messages[0])

        bad = dict(baseline, query_p99_ms=baseline["query_p99_ms"] * 3)
        self.assertEqual(
            check_regression.check(bad, baseline, 60, 150, ["query_p99_ms"]), []
        )

    def test_ignore_skips_comparison_but_validates(self):
        result = with_values(query_p99_ms=3000)
        self.assertEqual(
            check_regression.check(result, BASELINE, 20, 150, ["query_p99_ms"]), []
        )
        messages = check_regression.check(result, BASELINE, 20, 150)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])

        partial = dict(BASELINE)
        del partial["query_p99_ms"]
        with self.assertRaises(check_regression.InputError):
            check_regression.check(partial, BASELINE, 20, 150, ["query_p99_ms"])
        with self.assertRaises(check_regression.InputError):
            check_regression.check(result, BASELINE, 20, 150, ["nope"])

    def test_main_ignore_option(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            baseline_path = tmp / "baseline.json"
            baseline_path.write_text(json.dumps(BASELINE))

            result_path = tmp / "result.json"
            result_path.write_text(json.dumps(with_values(query_p99_ms=3000)) + "\n")
            self.assertEqual(
                check_regression.main(
                    [
                        str(result_path),
                        str(baseline_path),
                        "20",
                        "150",
                        "--ignore",
                        "query_p99_ms",
                    ]
                ),
                0,
            )
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "150"]
                ),
                1,
            )

            both_path = tmp / "both.json"
            both_path.write_text(
                json.dumps(with_values(query_p99_ms=3000, samples_per_second=100))
                + "\n"
            )
            self.assertEqual(
                check_regression.main(
                    [
                        str(both_path),
                        str(baseline_path),
                        "20",
                        "--ignore",
                        "query_p99_ms",
                    ]
                ),
                1,
            )
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "--ignore"]
                ),
                2,
            )
            self.assertEqual(
                check_regression.main(
                    [str(result_path), str(baseline_path), "20", "--ignore", "nope"]
                ),
                2,
            )

    def test_ci_log_check_uses_upper_threshold(self):
        ci = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
        text = ci.read_text(encoding="utf-8")
        self.assertIn(
            "tests/perf/baseline/log.json 60 150 --ignore query_p99_ms", text
        )
        self.assertIn("tests/perf/baseline/metric.json 60 160", text)
        self.assertNotIn("log.json 60 175", text)
        self.assertNotIn("log.json 60\n", text)

    def test_ci_metric_check_uses_upper_threshold(self):
        ci = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
        text = ci.read_text(encoding="utf-8")
        self.assertIn("tests/perf/baseline/metric.json 60 160", text)
        self.assertIn(
            "tests/perf/baseline/log.json 60 150 --ignore query_p99_ms", text
        )
        self.assertNotIn("metric.json 60 130", text)
        self.assertNotIn("metric.json 60\n", text)

    def _trace_baseline(self):
        path = Path(__file__).parent / "baseline" / "trace.json"
        return json.loads(path.read_text(encoding="utf-8"))

    def test_median_takes_median_of_each_key(self):
        m = median_result.median_result(TRACE_RUNS)
        self.assertEqual(m["cpu_seconds"], 35.0)
        self.assertEqual(m["query_p99_ms"], 710.0)
        self.assertEqual(m["samples_per_second"], 3800.0)
        self.assertEqual(m["peak_rss_kib"], 466000)
        self.assertEqual(m["runs"], 5)
        self.assertEqual(m["cpu_seconds_runs"], [32.65, 34.0, 49.81, 35.0, 53.4])
        self.assertEqual(m["samples_written"], 50000)

    def test_median_needs_odd_count_of_at_least_three(self):
        for runs in (TRACE_RUNS[:4], TRACE_RUNS[:2], TRACE_RUNS[:1]):
            with self.subTest(count=len(runs)):
                with self.assertRaises(median_result.InputError):
                    median_result.median_result(runs)
        median_result.median_result(TRACE_RUNS[:3])

    def test_median_rejects_bad_run(self):
        missing = [
            {k: v for k, v in run.items() if k != "cpu_seconds"} if i == 1 else run
            for i, run in enumerate(TRACE_RUNS)
        ]
        with self.assertRaises(median_result.InputError):
            median_result.median_result(missing)
        text = [dict(run) for run in TRACE_RUNS]
        text[1]["cpu_seconds"] = "x"
        with self.assertRaises(median_result.InputError):
            median_result.median_result(text)

    def test_median_main_writes_result_and_prints_runs(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = []
            for i, run in enumerate(TRACE_RUNS, start=1):
                path = Path(tmp) / f"r{i}.json"
                path.write_text(json.dumps(run) + "\n", encoding="utf-8")
                paths.append(str(path))
            out = Path(tmp) / "median.json"
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                code = median_result.main([str(out), *paths])
            self.assertEqual(code, 0)
            self.assertIn("median cpu_seconds=35.0", buf.getvalue())
            self.assertIn("run 5 cpu_seconds=53.4", buf.getvalue())
            written = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(written["cpu_seconds"], 35.0)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(
                    median_result.main([str(out), paths[0], paths[1]]), 2
                )
                self.assertEqual(
                    median_result.main([str(out), *paths[:4]]), 2
                )

    def test_trace_median_of_five_passes_at_60(self):
        median = median_result.median_result(TRACE_RUNS)
        self.assertEqual(
            check_regression.check(median, self._trace_baseline(), 60), []
        )

    def test_trace_single_slow_run_fails_at_60(self):
        messages = check_regression.check(TRACE_RUNS[4], self._trace_baseline(), 60)
        self.assertEqual(len(messages), 1)
        self.assertIn("cpu_seconds", messages[0])

    def test_trace_median_still_catches_large_regression(self):
        slow = [
            dict(run, cpu_seconds=run["cpu_seconds"] * 3, query_p99_ms=run["query_p99_ms"] * 3)
            for run in TRACE_RUNS
        ]
        median = median_result.median_result(slow)
        messages = check_regression.check(median, self._trace_baseline(), 60)
        self.assertEqual(len(messages), 2)
        self.assertTrue(any("cpu_seconds" in m for m in messages))
        self.assertTrue(any("query_p99_ms" in m for m in messages))

    def test_ci_trace_step_uses_median_and_threshold_60(self):
        ci = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
        text = ci.read_text(encoding="utf-8")
        self.assertIn("for i in 1 2 3 4 5; do", text)
        self.assertIn("tests/perf/median_result.py", text)
        lines = [line for line in text.splitlines() if "tests/perf/baseline/trace.json" in line]
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].rstrip().endswith("tests/perf/baseline/trace.json 60"))


if __name__ == "__main__":
    unittest.main()
