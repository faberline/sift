from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/release-profiles.md"


class ReleaseProfilesDocTest(unittest.TestCase):
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
                "## Profiles",
                "## Version and tag",
                "## Workflow design",
                "## Production profile is unchanged",
                "## Open decision",
            ],
        )

    def test_both_profiles_named(self):
        page = self.read(PAGE)
        self.assertIn("production", page)
        self.assertIn("single-node", page)

    def test_proposal_marker(self):
        self.assertIn(
            "Proposal: the owner confirms before any tag",
            self.read(PAGE),
        )

    def test_workflows_not_edited(self):
        for name in [
            ".github/workflows/sift-release.yml",
            ".github/workflows/sift-release-candidate.yml",
        ]:
            self.assertNotIn("single-node", self.read(name), name)

    def test_readme_links_the_page(self):
        self.assertIn(
            "operations/release-profiles.md",
            self.read("docs/README.md"),
        )


if __name__ == "__main__":
    unittest.main()
