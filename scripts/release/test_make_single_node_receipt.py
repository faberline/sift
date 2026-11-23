#!/usr/bin/env python3
"""Unit tests for scripts/release/make-single-node-receipt.py."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("make-single-node-receipt.py")
_spec = importlib.util.spec_from_file_location("make_single_node_receipt", SCRIPT)
receipt = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(receipt)

COMMIT = "a" * 40
DIGEST = "sha256:" + "b" * 64
GATE_FILES = {
    "G1": ("install_one_namespace", "install_one_namespace.json"),
    "G2": ("retention", "retention.json"),
    "G3": ("deleted_pod_logs", "deleted_pod_logs.json"),
}


def write_evidence(directory: Path) -> None:
    setup = {"phase": "setup", "image": "ghcr.io/faberline/sift@" + DIGEST}
    (directory / "setup.json").write_text(json.dumps(setup))
    for gate, (phase, name) in GATE_FILES.items():
        record = {
            "phase": phase,
            "gate": gate,
            "result": "pass",
            "selected": 5,
            "accepted": 5,
            "detail": "",
            "proves": "x",
            "does_not_prove": "y",
        }
        (directory / name).write_text(json.dumps(record))


def set_field(directory: Path, name: str, **changes: object) -> None:
    path = directory / name
    record = json.loads(path.read_text())
    record.update(changes)
    path.write_text(json.dumps(record))


class SingleNodeReceiptTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = Path(tmp.name)
        write_evidence(self.dir)

    def make(self, run_commit: str = COMMIT) -> dict:
        return receipt.make_receipt(self.dir, "123", "1", COMMIT, run_commit)

    def assert_refused(self, text: str, run_commit: str = COMMIT) -> None:
        with self.assertRaises(SystemExit) as caught:
            self.make(run_commit)
        message = str(caught.exception)
        self.assertTrue(message.startswith("single-node receipt refused: "), message)
        self.assertIn(text, message)

    def test_complete_evidence_gives_receipt(self) -> None:
        result = self.make()
        self.assertEqual(
            set(result.keys()),
            {"schema", "run_id", "attempt", "commit", "image_digest", "gates"},
        )
        self.assertEqual(result["run_id"], "123")
        self.assertEqual(result["attempt"], "1")
        self.assertEqual(result["commit"], COMMIT)
        self.assertEqual(result["image_digest"], DIGEST)
        self.assertEqual(list(result["gates"].keys()), ["G1", "G2", "G3"])
        for gate in ("G1", "G2", "G3"):
            self.assertEqual(result["gates"][gate]["result"], "pass")

    def test_missing_g2_fails(self) -> None:
        (self.dir / "retention.json").unlink()
        self.assert_refused("G2")

    def test_failed_g3_fails(self) -> None:
        set_field(self.dir, "deleted_pod_logs.json", result="fail")
        self.assert_refused("G3")

    def test_wrong_commit_fails(self) -> None:
        self.assert_refused("differs", run_commit="c" * 40)

    def test_zero_measurement_fails(self) -> None:
        set_field(self.dir, "install_one_namespace.json", selected=0, accepted=0)
        self.assert_refused("G1")

    def test_null_measurement_fails(self) -> None:
        set_field(self.dir, "retention.json", accepted=None)
        self.assert_refused("G2")

    def test_wrong_gate_field_fails(self) -> None:
        set_field(self.dir, "deleted_pod_logs.json", gate="G1")
        self.assert_refused("G3")

    def test_bad_image_fails(self) -> None:
        set_field(self.dir, "setup.json", image="ghcr.io/faberline/sift:latest")
        self.assert_refused("image digest")

    def test_partial_acceptance_is_refused(self) -> None:
        record = {
            "mode": "partial",
            "executed": ["install_one_namespace", "retention", "deleted_pod_logs"],
            "not_run": ["load_30m", "load_24h", "faults", "comparison"],
        }
        (self.dir / "acceptance.json").write_text(json.dumps(record))
        self.assert_refused("partial")

    def test_full_acceptance_file_is_accepted(self) -> None:
        record = {
            "mode": "full",
            "executed": [
                "install_one_namespace", "load_30m", "load_24h", "faults",
                "comparison", "retention", "deleted_pod_logs",
            ],
            "not_run": [],
        }
        (self.dir / "acceptance.json").write_text(json.dumps(record))
        self.assertEqual(self.make()["commit"], COMMIT)

    def test_cli_writes_output_and_refusal_writes_none(self) -> None:
        base = [
            sys.executable,
            "-B",
            str(SCRIPT),
            "--evidence-dir",
            str(self.dir),
            "--run-id",
            "123",
            "--attempt",
            "1",
            "--commit",
            COMMIT,
            "--run-commit",
            COMMIT,
        ]
        out = self.dir / "receipt.json"
        done = subprocess.run(base + ["--output", str(out)], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        written = json.loads(out.read_text())
        self.assertEqual(written["schema"], "sift.single-node-receipt/v1")

        (self.dir / "retention.json").unlink()
        refused_out = self.dir / "refused.json"
        done = subprocess.run(base + ["--output", str(refused_out)], capture_output=True, text=True)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(refused_out.exists())


if __name__ == "__main__":
    unittest.main()
