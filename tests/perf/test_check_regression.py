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

    def test_trace_baseline_has_every_compared_key(self):
        trace = json.loads(
            (Path(__file__).parent / "baseline" / "trace.json").read_text()
        )
        for key in [
            "samples_per_second",
            "query_p50_ms",
            "query_p95_ms",
            "query_p99_ms",
            "peak_rss_kib",
            "cpu_seconds",
        ]:
            self.assertIn(key, trace)
            self.assertIsInstance(trace[key], (int, float))
            self.assertGreater(trace[key], 0, key)

    def test_trace_baseline_fails_when_exceeded_and_passes_unchanged(self):
        trace = json.loads(
            (Path(__file__).parent / "baseline" / "trace.json").read_text()
        )
        self.assertEqual(check_regression.check(trace, trace, 60), [])
        exceeded = dict(trace, query_p99_ms=trace["query_p99_ms"] * 1.7)
        messages = check_regression.check(exceeded, trace, 60)
        self.assertEqual(len(messages), 1)
        self.assertIn("query_p99_ms", messages[0])


if __name__ == "__main__":
    unittest.main()
