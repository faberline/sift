"""Unit tests for scripts/verify-limits-doc.py."""

import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("verify-limits-doc.py")
spec = importlib.util.spec_from_file_location("verify_limits_doc", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

ROOT = Path(__file__).resolve().parents[1]

HEADER = "| Limit | Value | Enforced at |\n|---|---|---|\n"


class LimitsDocTest(unittest.TestCase):
    def test_limit_rows_parse(self):
        page = (
            "# t\n\n## Limits\n\n"
            + HEADER
            + "| A limit | 5 | `src/x.rs` `FOO` `BAR` |\n\n"
            + "## Known limits\n"
        )
        self.assertEqual(
            module.limit_rows(page),
            [("A limit", ["src/x.rs", "FOO", "BAR"])],
        )

    def test_limit_rows_without_table_raises(self):
        with self.assertRaises(ValueError):
            module.limit_rows("# t\n")

    def test_missing_file_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            errors = module.problems([("A", ["src/gone.rs", "FOO"])], Path(tmp))
        self.assertEqual(errors, ["A: file src/gone.rs does not exist"])

    def test_missing_item_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "x.rs").write_text("const FOO: u8 = 1;\n")
            errors = module.problems([("A", ["src/x.rs", "FOO", "BAR"])], root)
        self.assertEqual(errors, ["A: item BAR not found in src/x.rs"])

    def test_row_without_location_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            errors = module.problems([("A", [])], Path(tmp))
        self.assertEqual(errors, ["A: no enforcement location"])

    def test_real_page_passes(self):
        text = (ROOT / module.PAGE).read_text()
        rows = module.limit_rows(text)
        self.assertGreaterEqual(len(rows), 30)
        self.assertEqual(module.problems(rows, ROOT), [])

    def test_main_returns_nonzero_when_item_renamed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            page_path = root / module.PAGE
            page_path.parent.mkdir(parents=True)
            page_path.write_text((ROOT / module.PAGE).read_text())
            source = root / "src" / "ingest" / "domain"
            source.mkdir(parents=True)
            # An empty file: every item named for it is now missing.
            (source / "ingest_limits.rs").write_text("")
            with mock.patch.object(module, "ROOT", root):
                with contextlib.redirect_stderr(io.StringIO()):
                    code = module.main()
        self.assertEqual(code, 1)

    def test_readme_links_the_page(self):
        for name in ("README.md", "docs/README.md"):
            text = (ROOT / name).read_text()
            with self.subTest(file=name):
                self.assertIn("operations/limits.md", text)


if __name__ == "__main__":
    unittest.main()
