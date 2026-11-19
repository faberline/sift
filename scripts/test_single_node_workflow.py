from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ".github/workflows/sift-single-node-acceptance.yml"


class SingleNodeWorkflowTest(unittest.TestCase):
    def read(self, name):
        return (ROOT / name).read_text(encoding="utf-8")

    def test_manual_dispatch_only(self):
        text = self.read(WORKFLOW)
        self.assertIn("workflow_dispatch:", text)
        for trigger in ["push:", "pull_request:", "schedule:"]:
            self.assertNotIn(trigger, text)

    def test_dry_run_default_true(self):
        text = self.read(WORKFLOW)
        self.assertRegex(text, r"dry_run:\n(?:        .*\n)*?        default: true")

    def test_guard_uses_the_cost_guard(self):
        text = self.read(WORKFLOW)
        self.assertIn("cost_guard.py", text)
        self.assertIn("cost-inputs.single-node.json", text)

    def test_live_job_is_gated(self):
        text = self.read(WORKFLOW)
        self.assertIn("  live:", text)
        live = text.split("  live:", 1)[1]
        self.assertIn("needs: guard", live)
        self.assertIn("!inputs.dry_run", live)

    def test_no_secret_or_host(self):
        text = self.read(WORKFLOW)
        self.assertNotIn("secrets.", text)
        self.assertNotIn("gke_", text)


if __name__ == "__main__":
    unittest.main()
