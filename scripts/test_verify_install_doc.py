import importlib.util
import tempfile
import unittest
from pathlib import Path

# Without verify-install-doc.py this module fails to load, so every test fails first.
SPEC = importlib.util.spec_from_file_location(
    "verify_install_doc", Path(__file__).with_name("verify-install-doc.py")
)
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)
ROOT = verify.ROOT

GOOD = """# Install Sift

Four paths. The marker on each block says how its commands are tested.

## install.sh

<!-- manual: downloads a published release; no release is published yet -->
```sh
sh ./install.sh
```

## Container image

<!-- tested: tests/x.sh -->
```sh
docker build --file Dockerfile --tag sift:dev .
```

## Docker Compose

<!-- manual: CI does not run docker compose -->
```sh
docker compose -f compose.yaml up --build
```

## Kubernetes

<!-- tested: tests/x.sh -->
```sh
kubectl kustomize k8s/overlays/template
```
"""


def block_line(page, marker):
    # The sh fence comes two lines after the marker: marker, blank-free fence.
    lines = page.splitlines()
    index = next(i for i, line in enumerate(lines) if marker in line)
    return index + 2


class InstallDocTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "tests").mkdir()
        (self.root / "tests" / "x.sh").write_text("#!/bin/sh\n")
        self.ci_text = "run: sh tests/x.sh\n"

    def tearDown(self):
        self._tmp.cleanup()

    def test_good_page_has_no_problems(self):
        self.assertEqual(verify.problems(GOOD, self.root, self.ci_text), [])

    def test_block_without_marker_fails(self):
        page = GOOD.replace(
            "<!-- manual: downloads a published release; no release is published yet -->\n",
            "",
        )
        line = page.splitlines().index("```sh") + 1
        self.assertEqual(
            verify.problems(page, self.root, self.ci_text),
            [f"line {line}: sh block has no tested or manual marker"],
        )

    def test_missing_heading_fails(self):
        page = GOOD.replace("## Kubernetes\n", "")
        self.assertEqual(
            verify.problems(page, self.root, self.ci_text),
            ["missing heading ## Kubernetes"],
        )

    def test_short_manual_reason_fails(self):
        page = GOOD.replace(
            "downloads a published release; no release is published yet", "no"
        )
        self.assertEqual(
            verify.problems(page, self.root, self.ci_text),
            [f"line {block_line(page, '<!-- manual: no -->')}: manual reason too short"],
        )

    def test_tested_file_missing_fails(self):
        page = GOOD.replace("tests/x.sh", "tests/gone.sh", 1)
        self.assertEqual(
            verify.problems(page, self.root, self.ci_text),
            [
                f"line {block_line(page, '<!-- tested: tests/gone.sh -->')}: "
                "tested file tests/gone.sh does not exist"
            ],
        )

    def test_tested_file_not_in_ci_fails(self):
        found = verify.problems(GOOD, self.root, "")
        self.assertEqual(len(found), 2)
        for message in found:
            self.assertIn("is not run by ci.yml", message)

    def test_sh_blocks_marker_is_nearest_non_blank_line(self):
        page = "<!-- manual: reason reason -->\n\n```sh\nx\n```\n"
        self.assertEqual(
            verify.sh_blocks(page), [(3, "<!-- manual: reason reason -->")]
        )

    def test_real_page_passes(self):
        page = (ROOT / verify.PAGE).read_text(encoding="utf-8")
        ci_text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertEqual(verify.problems(page, ROOT, ci_text), [])

    def test_docs_readme_links_the_page(self):
        readme = (ROOT / "docs/README.md").read_text(encoding="utf-8")
        self.assertIn("operations/install.md", readme)


if __name__ == "__main__":
    unittest.main()
