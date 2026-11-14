import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location(
    "verify_alert_runbook", Path(__file__).with_name("verify-alert-runbook.py")
)
vr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(vr)

RULE_RS = (ROOT / "src/alerting/domain/rule.rs").read_text(encoding="utf-8")
RUNBOOK = (ROOT / "docs/operations/runbook.md").read_text(encoding="utf-8")


def complete_entry(kind, first_command="sift control alerts list --project p"):
    return (
        f"**Meaning**: The {kind} alert.\n\n"
        "**First command**\n\n"
        "```sh\n"
        f"{first_command}\n"
        "```\n\n"
        "**Next step**: Read the state and fix the cause.\n\n"
        "**Verified**: manual - needs a live endpoint and a token.\n"
    )


def runbook_with(kinds):
    parts = ["## Alerts\n", "Sift ships no fixed alert rules.\n"]
    for kind in kinds:
        parts.append(f"### {kind}\n\n{complete_entry(kind)}\n")
    parts.append("Source: `src/alerting/domain/rule.rs`.\n")
    return "\n".join(parts)


class AlertRunbookTest(unittest.TestCase):
    def test_shipped_alerts_from_the_real_source(self):
        self.assertEqual(
            vr.shipped_alerts(RULE_RS),
            ["metrics", "logs_count", "new_error", "error_recurrence"],
        )

    def test_shipped_alerts_snake_case(self):
        text = "pub enum RuleSource {\n    FooBar { x: u8 },\n    Baz,\n}\n"
        self.assertEqual(vr.shipped_alerts(text), ["foo_bar", "baz"])

    def test_shipped_alerts_missing_enum(self):
        with self.assertRaises(ValueError):
            vr.shipped_alerts("fn f() {}")

    def test_real_runbook_is_complete(self):
        kinds = vr.shipped_alerts(RULE_RS)
        self.assertEqual(vr.problems(kinds, vr.runbook_entries(RUNBOOK)), [])

    def test_alert_without_entry_fails(self):
        # Without the runbook entry for new_error, this test fails.
        entries = {"metrics": complete_entry("metrics")}
        self.assertEqual(
            vr.problems(["metrics", "new_error"], entries),
            ["alert new_error: no runbook entry"],
        )

    def test_missing_label_and_code_block(self):
        without_next = complete_entry("metrics").replace(
            "**Next step**: Read the state and fix the cause.\n\n", ""
        )
        self.assertEqual(
            vr.problems(["metrics"], {"metrics": without_next}),
            ["alert metrics: missing **Next step**"],
        )

        no_block = (
            "**Meaning**: x\n\n**First command**\n\n"
            "**Next step**: y\n\n**Verified**: manual - z\n"
        )
        self.assertIn(
            "alert metrics: first command has no code block",
            vr.problems(["metrics"], {"metrics": no_block}),
        )

    def test_runbook_entries_stops_at_next_h2(self):
        entries = vr.runbook_entries("## Alerts\n\n### a\nx\n\n## Other\n\n### b\ny\n")
        self.assertEqual(list(entries.keys()), ["a"])

    def test_main_returns_nonzero_when_entry_removed(self):
        kinds = ["metrics", "logs_count", "error_recurrence"]
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "src/alerting/domain").mkdir(parents=True)
            (tmp / "src/alerting/domain/rule.rs").write_text(RULE_RS, encoding="utf-8")
            (tmp / "docs/operations").mkdir(parents=True)
            (tmp / "docs/operations/runbook.md").write_text(
                runbook_with(kinds), encoding="utf-8"
            )
            err = io.StringIO()
            with mock.patch.object(vr, "ROOT", tmp), contextlib.redirect_stderr(err):
                self.assertEqual(vr.main(), 1)
            self.assertIn("alert new_error: no runbook entry", err.getvalue())

            shutil.rmtree(tmp / "docs")
            (tmp / "docs/operations").mkdir(parents=True)
            (tmp / "docs/operations/runbook.md").write_text(
                runbook_with(["metrics", "logs_count", "new_error", "error_recurrence"]),
                encoding="utf-8",
            )
            with mock.patch.object(vr, "ROOT", tmp), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(vr.main(), 0)


if __name__ == "__main__":
    unittest.main()
