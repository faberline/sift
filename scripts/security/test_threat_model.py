"""Tests for the threat model checker (scripts/security/threat_model.py)."""

import unittest
from pathlib import Path

import threat_model

ROOT = Path(__file__).resolve().parents[2]
PAGE = (ROOT / "docs/security/threat-model.md").read_text()


def replace_cell(page, start, index, value):
    """Replace one cell (split on '|') of each line that starts with `start`."""
    out = []
    for line in page.splitlines():
        if line.startswith(start):
            cells = line.split("|")
            cells[index] = value
            line = "|".join(cells)
        out.append(line)
    return "\n".join(out)


class ThreatModel(unittest.TestCase):
    def test_real_page_has_no_error(self):
        self.assertEqual(threat_model.check(PAGE, ROOT), [])

    def test_missing_boundary_row_fails(self):
        page = "\n".join(
            line for line in PAGE.splitlines() if not line.startswith("| Secrets |")
        )
        self.assertIn("missing boundary: Secrets", threat_model.check(page, ROOT))

    def test_row_without_control_fails(self):
        page = replace_cell(PAGE, "| Image supply chain |", 2, " ")
        self.assertIn(
            "no control: Image supply chain", threat_model.check(page, ROOT)
        )

    def test_row_without_proof_fails(self):
        page = replace_cell(PAGE, "| Webhook receiver |", 3, " ")
        self.assertIn("no proof: Webhook receiver", threat_model.check(page, ROOT))

    def test_unknown_test_name_fails(self):
        token = "`tests/it/runtime_security_e2e.rs::a_token_is_bound_to_its_project`"
        page = PAGE.replace(token, "`tests/it/runtime_security_e2e.rs::no_such_test`")
        self.assertIn(
            "proof not found: tests/it/runtime_security_e2e.rs::no_such_test",
            threat_model.check(page, ROOT),
        )

    def test_unknown_test_file_fails(self):
        token = "`tests/it/runtime_security_e2e.rs::a_token_is_bound_to_its_project`"
        page = PAGE.replace(token, "`tests/it/no_such_file.rs::x`")
        self.assertIn(
            "proof not found: tests/it/no_such_file.rs::x",
            threat_model.check(page, ROOT),
        )

    def test_missing_review_section_fails(self):
        page = "\n".join(
            line
            for line in PAGE.splitlines()
            if line != "## Independent security review"
        )
        self.assertIn(
            "missing section: Independent security review",
            threat_model.check(page, ROOT),
        )

    def test_review_must_not_be_claimed_done(self):
        page = PAGE.replace("not claimed", "done")
        self.assertIn(
            "review must say: not claimed", threat_model.check(page, ROOT)
        )

    def test_page_is_linked_from_docs_readme(self):
        readme = (ROOT / "docs/README.md").read_text()
        self.assertIn("(security/threat-model.md)", readme)


if __name__ == "__main__":
    unittest.main()
