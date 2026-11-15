import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs/operations/install.md"
CI = ROOT / ".github/workflows/ci.yml"

_spec = importlib.util.spec_from_file_location(
    "verify_install_doc", Path(__file__).with_name("verify-install-doc.py")
)
m = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(m)

# Header lines 1-10 hold the four required headings; the body starts at line 11.
HEADER = (
    "# Install Sift\n\n"
    "## install.sh\n\n"
    "## Container image\n\n"
    "## Docker Compose\n\n"
    "## Kubernetes\n\n"
)
GOOD = HEADER + (
    "<!-- tested: tests/x.sh -->\n"
    "```sh\n"
    "sh tests/x.sh\n"
    "```\n"
)


class InstallDocTest(unittest.TestCase):
    def _root(self, tmp):
        root = Path(tmp)
        (root / "tests").mkdir()
        (root / "tests/x.sh").write_text("#!/bin/sh\n")
        return root

    def test_good_page_has_no_problems(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(GOOD, self._root(tmp), "x.sh")
        self.assertEqual(problems, [])

    def test_block_without_marker_fails(self):
        page = HEADER + "```sh\nx\n```\n"
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(page, self._root(tmp), "x.sh")
        self.assertEqual(
            problems, ["line 11: sh block has no tested or manual marker"]
        )

    def test_missing_heading_fails(self):
        page = GOOD.replace("## Kubernetes\n", "")
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(page, self._root(tmp), "x.sh")
        self.assertEqual(problems, ["missing heading ## Kubernetes"])

    def test_short_manual_reason_fails(self):
        page = HEADER + "<!-- manual: no -->\n```sh\nx\n```\n"
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(page, self._root(tmp), "x.sh")
        self.assertEqual(problems, ["line 12: manual reason too short"])

    def test_tested_file_missing_fails(self):
        page = HEADER + "<!-- tested: tests/gone.sh -->\n```sh\nx\n```\n"
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(page, self._root(tmp), "gone.sh")
        self.assertEqual(
            problems, ["line 12: tested file tests/gone.sh does not exist"]
        )

    def test_tested_file_not_in_ci_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems(GOOD, self._root(tmp), "")
        self.assertEqual(
            problems, ["line 12: tested file tests/x.sh is not run by ci.yml"]
        )

    def test_sh_blocks_marker_is_nearest_non_blank_line(self):
        page = "<!-- manual: reason reason -->\n\n```sh\nx\n```\n"
        self.assertEqual(
            m.sh_blocks(page), [(3, "<!-- manual: reason reason -->")]
        )

    def test_real_page_passes(self):
        page = PAGE.read_text(encoding="utf-8")
        ci_text = CI.read_text(encoding="utf-8")
        self.assertEqual(m.problems(page, ROOT, ci_text), [])

    def test_docs_readme_links_the_page(self):
        text = (ROOT / "docs/README.md").read_text(encoding="utf-8")
        self.assertIn("operations/install.md", text)


if __name__ == "__main__":
    unittest.main()
