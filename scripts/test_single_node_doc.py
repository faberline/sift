from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/single-node-v1.md"


class SingleNodeDocTest(unittest.TestCase):
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
                "## Scope",
                "## Promises",
                "## Does not promise",
                "## Acceptance gates",
                "## Senders",
                "## Capacity",
                "## Relation to production-v1",
            ],
        )

    def test_gates_g1_to_g4_listed(self):
        page = self.read(PAGE)
        for gate in ["G1", "G2", "G3", "G4"]:
            self.assertIn(gate, page)

    def test_readme_links_the_page(self):
        self.assertIn("single-node-v1.md", self.read("docs/README.md"))

    def test_no_unproven_claim(self):
        page = self.read(PAGE).lower()
        for word in ["production-ready", "passed", "verified"]:
            self.assertNotIn(word, page)

    def test_gates_are_not_run(self):
        page = self.read(PAGE)
        self.assertIn("not run", page.lower())
        self.assertIn("No live GKE evidence exists yet", page)

    def test_lock_restart_is_stated(self):
        page = self.read(PAGE)
        self.assertIn("does not wait", page)
        self.assertIn("another Sift process may be using it", page)

    def test_g4_names_existing_ci_jobs(self):
        page = self.read(PAGE)
        ci = self.read(".github/workflows/ci.yml")
        for name in ["test: compat", "test: perf (metric workload)"]:
            self.assertIn(name, page)
            self.assertIn(f'name: "{name}"', ci)


if __name__ == "__main__":
    unittest.main()
