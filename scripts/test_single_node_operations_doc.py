from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/single-node.md"


class SingleNodeOperationsDocTest(unittest.TestCase):
    def read(self, name):
        return (ROOT / name).read_text(encoding="utf-8")

    def test_sections_in_order(self):
        headings = [
            line
            for line in self.read(PAGE).splitlines()
            if line.startswith("## ")
        ]
        self.assertEqual(
            headings,
            [
                "## When to use it",
                "## Install",
                "## What the operator renders",
                "## Size the PVC",
                "## What is lost against the three-voter topology",
                "## Related",
            ],
        )

    def test_title(self):
        first = self.read(PAGE).splitlines()[0]
        self.assertEqual(first, "# Run Sift as one Pod (single-node topology)")

    def test_example_sets_single_node(self):
        page = self.read(PAGE)
        self.assertIn("topology: singleNode", page)
        self.assertIn("storeSize:", page)
        self.assertNotIn("replicasPerShard", page)
        self.assertNotIn("voterCount", page)

    def test_names_each_lost_guarantee(self):
        page = self.read(PAGE)
        for phrase in [
            "**No quorum**",
            "**No failover**",
            "**Unavailable while it restarts**",
            "**Recovery only from the PVC or a GCS archive or backup**",
        ]:
            self.assertIn(phrase, page)

    def test_states_supported_production_mode(self):
        page = self.read(PAGE).lower()
        self.assertIn("supported production mode", page)
        self.assertIn("not only a trial mode", page)

    def test_no_unproven_claim(self):
        page = self.read(PAGE).lower()
        for word in ["production-ready", "passed", "verified"]:
            self.assertNotIn(word, page)

    def test_no_secret_or_personal_path(self):
        page = self.read(PAGE)
        for token in ["/home/", "/root/", "@"]:
            self.assertNotIn(token, page)

    def test_readme_links_the_page(self):
        self.assertIn("docs/operations/single-node.md", self.read("README.md"))

    def test_ha_links_the_page(self):
        self.assertIn("](single-node.md)", self.read("docs/operations/ha.md"))

    def test_links_resolve(self):
        page_dir = (ROOT / PAGE).parent
        page = self.read(PAGE)
        targets = re.findall(r"\]\(([^)]+)\)", page)
        for target in ["../single-node-v1.md", "ha.md", "../production-v1.md"]:
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

    def test_facts_match_the_code(self):
        self.assertIn("singleNode", self.read("k8s/crd/sift.yaml"))
        self.assertIn(
            "fn render_single_node",
            self.read("src/operations/infrastructure/k8s/managed_service.rs"),
        )


if __name__ == "__main__":
    unittest.main()
