"""Unit tests for scripts/verify-install-doc.py."""

import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("verify-install-doc.py")
spec = importlib.util.spec_from_file_location("verify_install_doc", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

ROOT = Path(__file__).resolve().parents[1]

CI_TEXT = "      run: bash tests/x.sh\n"


def build_page(install_marker="manual: downloads a release; none is published yet",
               container_marker="tested: tests/x.sh",
               kube_marker="tested: tests/x.sh",
               headings=("## install.sh", "## Container image", "## Docker Compose", "## Kubernetes")):
    parts = ["# Install Sift", "", "Four paths.", ""]
    sections = [
        ("## install.sh", install_marker, "sh ./install.sh"),
        ("## Container image", container_marker, "docker build --file Dockerfile ."),
        ("## Docker Compose", "manual: CI does not run docker compose", "docker compose up"),
        ("## Kubernetes", kube_marker, "kubectl kustomize k8s/overlays/template"),
    ]
    for heading, marker, command in sections:
        if heading not in headings:
            continue
        parts.append(heading)
        parts.append("")
        if marker is not None:
            parts.append(f"<!-- {marker} -->")
        parts += ["```sh", command, "```", ""]
    return "\n".join(parts)


def sh_line(page, nth):
    """Line number (1-based) of the nth opening ```sh fence."""
    fences = [i + 1 for i, line in enumerate(page.splitlines()) if line == "```sh"]
    return fences[nth]


class InstallDocTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "tests").mkdir()
        (self.root / "tests" / "x.sh").write_text("#!/bin/sh\n")

    def tearDown(self):
        self._tmp.cleanup()

    def test_good_page_has_no_problems(self):
        page = build_page()
        self.assertEqual(module.problems(page, self.root, CI_TEXT), [])

    def test_block_without_marker_fails(self):
        page = build_page(install_marker=None)
        errors = module.problems(page, self.root, CI_TEXT)
        self.assertEqual(
            errors,
            [f"line {sh_line(page, 0)}: sh block has no tested or manual marker"],
        )

    def test_missing_heading_fails(self):
        page = build_page(headings=("## install.sh", "## Container image", "## Docker Compose"))
        errors = module.problems(page, self.root, CI_TEXT)
        self.assertEqual(errors, ["missing heading ## Kubernetes"])

    def test_short_manual_reason_fails(self):
        page = build_page(install_marker="manual: no")
        errors = module.problems(page, self.root, CI_TEXT)
        self.assertEqual(errors, [f"line {sh_line(page, 0)}: manual reason too short"])

    def test_tested_file_missing_fails(self):
        page = build_page(container_marker="tested: tests/gone.sh")
        errors = module.problems(page, self.root, CI_TEXT)
        self.assertEqual(
            errors,
            [f"line {sh_line(page, 1)}: tested file tests/gone.sh does not exist"],
        )

    def test_tested_file_not_in_ci_fails(self):
        page = build_page()
        errors = module.problems(page, self.root, "")
        self.assertEqual(
            errors,
            [
                f"line {sh_line(page, 1)}: tested file tests/x.sh is not run by ci.yml",
                f"line {sh_line(page, 3)}: tested file tests/x.sh is not run by ci.yml",
            ],
        )

    def test_sh_blocks_marker_is_nearest_non_blank_line(self):
        page = "<!-- manual: reason reason -->\n\n```sh\nx\n```\n"
        self.assertEqual(
            module.sh_blocks(page),
            [(3, "<!-- manual: reason reason -->")],
        )

    def test_real_page_passes(self):
        page = (ROOT / module.PAGE).read_text()
        ci_text = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertEqual(module.problems(page, ROOT, ci_text), [])

    def test_docs_readme_links_the_page(self):
        text = (ROOT / "docs/README.md").read_text()
        self.assertIn("operations/install.md", text)


if __name__ == "__main__":
    unittest.main()
