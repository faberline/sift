import json
import tempfile
import unittest
from pathlib import Path

import check_regression

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
    "query_p50_ms": 428.203,
    "query_p95_ms": 464.051,
    "query_p99_ms": 954.58,
    "peak_rss_kib": 291648,
    "cpu_seconds": 44.78,
}


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
        self.assertEqual(check_regression.check(SLOW_RUN, baseline, 60, 130), [])
        self.assertNotEqual(check_regression.check(SLOW_RUN, baseline, 60), [])
        self.assertEqual(check_regression.check(baseline, baseline, 60, 130), [])

    def test_metric_baseline_still_catches_large_regression(self):
        path = Path(__file__).parent / "baseline" / "metric.json"
        baseline = json.loads(path.read_text(encoding="utf-8"))

        bad = dict(baseline, cpu_seconds=baseline["cpu_seconds"] * 3)
        messages = check_regression.check(bad, baseline, 60, 130)
        self.assertEqual(len(messages), 1)
        self.assertIn("cpu_seconds", messages[0])

        bad = dict(baseline, query_p99_ms=baseline["query_p99_ms"] * 3)
        messages = check_regression.check(bad, baseline, 60, 130)
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

    def test_ci_metric_check_uses_upper_threshold(self):
        ci = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
        text = ci.read_text(encoding="utf-8")
        self.assertIn("tests/perf/baseline/metric.json 60 130", text)
        self.assertIn("tests/perf/baseline/log.json 60", text)
        self.assertNotIn("metric.json 60\n", text)


if __name__ == "__main__":
    unittest.main()
