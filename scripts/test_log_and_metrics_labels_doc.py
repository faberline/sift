from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/log-and-metrics-labels.md"


class LogAndMetricsLabelsDocTest(unittest.TestCase):
    def read(self, name):
        return (ROOT / name).read_text(encoding="utf-8")

    def test_title(self):
        first = self.read(PAGE).splitlines()[0]
        self.assertEqual(first, "# Collect logs and scrape metrics for one Deployment")

    def test_sections_in_order(self):
        headings = [
            line
            for line in self.read(PAGE).splitlines()
            if line.startswith("## ")
        ]
        self.assertEqual(
            headings,
            [
                "## The two labels",
                "## Example Deployment",
                "## Example Sift resource",
                "## Namespaces and the label work together",
                "## When the label is added or removed",
                "## Permissions",
                "## Check it",
            ],
        )

    def test_names_both_labels_and_the_field(self):
        page = self.read(PAGE)
        self.assertIn('sift.axiom.dev/logs: "true"', page)
        self.assertIn('sift.axiom.dev/scrape: "true"', page)
        self.assertIn("spec.logs.podLabel", page)
        self.assertIn("sift.axiom.dev/logs=true", page)
        self.assertIn("prometheus.io/scrape", page)

    def test_namespace_rule_is_stated(self):
        page = self.read(PAGE)
        self.assertIn("in a listed namespace", page)
        self.assertIn("carries the label", page)

    def test_late_label_behavior(self):
        page = self.read(PAGE)
        self.assertIn("5 seconds", page)
        self.assertIn("from the start of its current log file", page)
        self.assertIn("stops being collected", page)

    def test_rbac_sentence(self):
        page = self.read(PAGE)
        self.assertIn("<name>-agent-project", page)
        self.assertIn("get", page)
        self.assertIn("list", page)
        self.assertIn("no cluster-wide role", page)

    def test_no_unproven_claim(self):
        page = self.read(PAGE).lower()
        for word in ["production-ready", "passed", "verified"]:
            self.assertNotIn(word, page)

    def test_no_secret_or_personal_path(self):
        page = self.read(PAGE)
        for text in ["/home/", "/root/", "@"]:
            self.assertNotIn(text, page)

    def test_example_matches_the_crd(self):
        self.assertIn("podLabel:", self.read("k8s/crd/sift.yaml"))
        self.assertIn(
            "POD_LABEL_REFRESH: Duration = Duration::from_secs(5)",
            self.read("src/collector/infrastructure/pod_label_watch.rs"),
        )
        self.assertIn(
            "--pod-label",
            self.read("src/operations/infrastructure/k8s/agent_workloads.rs"),
        )

    def test_links_resolve(self):
        page_dir = (ROOT / PAGE).parent
        page = self.read(PAGE)
        targets = re.findall(r"\]\(([^)]+)\)", page)
        for target in ["metrics-scrape.md", "configuration.md"]:
            self.assertIn(target, targets)
        for target in targets:
            if target.startswith("http"):
                continue
            path = target.split("#", 1)[0]
            if not path:
                continue
            self.assertTrue(
                (page_dir / path).resolve().is_file(),
                f"broken link: {target}",
            )

    def test_indexed_and_linked(self):
        self.assertIn(
            "operations/log-and-metrics-labels.md",
            self.read("docs/README.md"),
        )
        self.assertIn(
            "](log-and-metrics-labels.md)",
            self.read("docs/operations/metrics-scrape.md"),
        )

    def test_ci_runs_this_test(self):
        self.assertIn(
            "scripts/test_log_and_metrics_labels_doc.py",
            self.read(".github/workflows/ci.yml"),
        )


if __name__ == "__main__":
    unittest.main()
