import contextlib
import io
import json
import os
import tempfile
import unittest
from pathlib import Path

import check_regression
import compare_trace

HERE = Path(__file__).resolve().parent
CI_YML = HERE.parents[1] / ".github" / "workflows" / "ci.yml"

BASE = {
    "samples_per_second": 4000.0,
    "query_p50_ms": 700.0,
    "query_p95_ms": 710.0,
    "query_p99_ms": 720.0,
    "peak_rss_kib": 466000,
    "cpu_seconds": 100.0,
}


def head_with(**changes):
    result = dict(BASE)
    result.update(changes)
    return result


def write_json(directory, name, value):
    path = Path(directory) / name
    path.write_text(json.dumps(value) + "\n", encoding="utf-8")
    return str(path)


class CompareTrace(unittest.TestCase):
    def run_main(self, head, base, threshold="60"):
        with tempfile.TemporaryDirectory() as tmp:
            head_file = write_json(tmp, "head.json", head)
            base_file = write_json(tmp, "base.json", base)
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = compare_trace.main([head_file, base_file, threshold])
            return code, out.getvalue(), err.getvalue()

    def test_rows_have_ratio(self):
        rows = compare_trace.comparison_rows(head_with(cpu_seconds=125.0), BASE)
        self.assertEqual(len(rows), 6)
        self.assertEqual(
            [row[0] for row in rows], list(check_regression.KEYS)
        )
        self.assertEqual(rows[check_regression.KEYS.index("cpu_seconds")],
                         ("cpu_seconds", 100.0, 125.0, 1.25))

    def test_equal_head_and_base_pass(self):
        code, _, _ = self.run_main(head_with(), BASE)
        self.assertEqual(code, 0)

    def test_exact_limit_passes_and_over_limit_fails(self):
        code, _, _ = self.run_main(head_with(cpu_seconds=150.0), BASE)
        self.assertEqual(code, 0)
        code, _, err = self.run_main(head_with(cpu_seconds=161.0), BASE)
        self.assertEqual(code, 1)
        self.assertIn("cpu_seconds", err)

    def test_throughput_below_forty_percent_fails(self):
        code, _, _ = self.run_main(head_with(samples_per_second=1599.0), BASE)
        self.assertEqual(code, 1)
        code, _, _ = self.run_main(head_with(samples_per_second=1601.0), BASE)
        self.assertEqual(code, 0)

    def test_slow_job_passes_against_same_job_base_but_fails_stored_baseline(self):
        stored = json.loads(
            (HERE / "baseline" / "trace.json").read_text(encoding="utf-8")
        )
        factor = 1.55
        slow_base = {
            "samples_per_second": stored["samples_per_second"] / factor,
            "query_p50_ms": stored["query_p50_ms"] * factor,
            "query_p95_ms": stored["query_p95_ms"] * factor,
            "query_p99_ms": stored["query_p99_ms"] * factor,
            "peak_rss_kib": stored["peak_rss_kib"],
            "cpu_seconds": 53.2,
        }
        slow_head = dict(slow_base, cpu_seconds=53.31)
        code, _, _ = self.run_main(slow_head, slow_base)
        self.assertEqual(code, 0)
        # Negative case: the stored baseline rule fails this same head result.
        messages = check_regression.check(slow_head, stored, 60)
        self.assertTrue(messages)
        self.assertTrue(any("cpu_seconds" in m for m in messages))

    def test_summary_file_has_both_medians_and_ratio(self):
        with tempfile.TemporaryDirectory() as tmp:
            summary = os.path.join(tmp, "summary.md")
            old = os.environ.get("GITHUB_STEP_SUMMARY")
            os.environ["GITHUB_STEP_SUMMARY"] = summary
            self.addCleanup(
                lambda: os.environ.pop("GITHUB_STEP_SUMMARY", None)
                if old is None
                else os.environ.__setitem__("GITHUB_STEP_SUMMARY", old)
            )
            head_file = write_json(tmp, "head.json", head_with(cpu_seconds=125.0))
            base_file = write_json(tmp, "base.json", BASE)
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = compare_trace.main([head_file, base_file, "60"])
            self.assertEqual(code, 0)
            text = Path(summary).read_text(encoding="utf-8")
            self.assertIn("| key | base median | head median | ratio |", text)
            self.assertIn("100.0", text)
            self.assertIn("125.0", text)
            self.assertIn("1.250", text)
            self.assertIn("result: pass", text)
            self.assertIn("cpu_seconds base=100.0 head=125.0 ratio=1.250", out.getvalue())

    def test_summary_file_error_does_not_change_exit_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            old = os.environ.get("GITHUB_STEP_SUMMARY")
            os.environ["GITHUB_STEP_SUMMARY"] = os.path.join(tmp, "missing", "s.md")
            self.addCleanup(
                lambda: os.environ.pop("GITHUB_STEP_SUMMARY", None)
                if old is None
                else os.environ.__setitem__("GITHUB_STEP_SUMMARY", old)
            )
            head_file = write_json(tmp, "head.json", head_with())
            base_file = write_json(tmp, "base.json", BASE)
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = compare_trace.main([head_file, base_file, "60"])
            self.assertEqual(code, 0)
            self.assertIn("warning: cannot write the job summary", err.getvalue())

    def test_bad_input_exits_2(self):
        with tempfile.TemporaryDirectory() as tmp:
            head_file = write_json(tmp, "head.json", head_with())
            base_file = write_json(tmp, "base.json", BASE)
            missing = os.path.join(tmp, "missing.json")
            no_cpu = write_json(
                tmp, "no_cpu.json", {k: v for k, v in head_with().items() if k != "cpu_seconds"}
            )
            cases = [
                [head_file, base_file],
                [head_file, base_file, "60", "extra"],
                [missing, base_file, "60"],
                [head_file, base_file, "abc"],
                [no_cpu, base_file, "60"],
            ]
            for args in cases:
                err = io.StringIO()
                with self.subTest(args=args), contextlib.redirect_stderr(err), \
                        contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(compare_trace.main(args), 2)
                self.assertTrue(err.getvalue())

    def test_ci_trace_step_compares_base_and_head(self):
        text = CI_YML.read_text(encoding="utf-8")
        self.assertIn("for i in 1 2 3 4 5; do", text)
        self.assertIn("tests/perf/median_result.py", text)
        self.assertIn("HEAD^", text)
        self.assertIn("fetch-depth: 2", text)
        self.assertIn("test_check_regression test_trace_compare", text)
        lines = [line for line in text.splitlines() if "tests/perf/compare_trace.py" in line]
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].rstrip().endswith('perf-trace-base.json" 60'))
        self.assertNotIn("baseline/trace.json", text)
        loop_start = text.index("for i in 1 2 3 4 5; do")
        base_at = text.index("perf-trace-base-$i.json", loop_start)
        head_at = text.index("perf-trace-head-$i.json", loop_start)
        self.assertLess(base_at, head_at)


if __name__ == "__main__":
    unittest.main()
