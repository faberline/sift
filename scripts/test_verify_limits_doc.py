import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/limits.md"

_spec = importlib.util.spec_from_file_location(
    "verify_limits_doc", Path(__file__).with_name("verify-limits-doc.py")
)
m = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(m)


class LimitsDocTest(unittest.TestCase):
    def test_limit_rows_parse(self):
        page = (
            "# t\n\n## Limits\n\n"
            "| Limit | Value | Enforced at |\n"
            "|---|---|---|\n"
            "| A limit | 5 | `src/x.rs` `FOO` `BAR` |\n\n"
            "## Known limits\n"
        )
        self.assertEqual(
            m.limit_rows(page), [("A limit", ["src/x.rs", "FOO", "BAR"])]
        )

    def test_limit_rows_without_table_raises(self):
        with self.assertRaises(ValueError):
            m.limit_rows("# t\n")

    def test_missing_file_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems([("A", ["src/gone.rs", "FOO"])], Path(tmp))
        self.assertEqual(problems, ["A: file src/gone.rs does not exist"])

    def test_missing_item_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src/x.rs").write_text("const FOO: u8 = 1;\n")
            problems = m.problems([("A", ["src/x.rs", "FOO", "BAR"])], root)
        self.assertEqual(problems, ["A: item BAR not found in src/x.rs"])

    def test_row_without_location_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = m.problems([("A", [])], Path(tmp))
        self.assertEqual(problems, ["A: no enforcement location"])

    def test_real_page_passes(self):
        rows = m.limit_rows((ROOT / PAGE).read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(rows), 30)
        self.assertEqual(m.problems(rows, ROOT), [])

    def test_main_returns_nonzero_when_item_renamed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / PAGE).parent.mkdir(parents=True)
            shutil.copy(ROOT / PAGE, root / PAGE)
            source = root / "src/ingest/domain/ingest_limits.rs"
            source.parent.mkdir(parents=True)
            source.write_text("")
            with mock.patch.object(m, "ROOT", root), contextlib.redirect_stderr(
                io.StringIO()
            ):
                self.assertEqual(m.main(), 1)

    def test_readme_links_the_page(self):
        for name in ("README.md", "docs/README.md"):
            with self.subTest(file=name):
                text = (ROOT / name).read_text(encoding="utf-8")
                self.assertIn("operations/limits.md", text)


if __name__ == "__main__":
    unittest.main()
