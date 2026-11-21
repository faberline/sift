#!/usr/bin/env python3
"""Make a single-node release receipt from one sift-single-node-acceptance run.

The receipt holds the run id, the attempt, the source commit, the image digest
and the result of gates G1 to G3. The script refuses (non-zero exit, message
names the gate) a run with a failed or missing gate, a null or zero measurement,
or a commit that differs from the commit of the run. A refusal writes no file.

This script reads evidence files only. It never reads credentials.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

SCHEMA = "sift.single-node-receipt/v1"
GATE_FILES = {
    "G1": "install_one_namespace.json",
    "G2": "retention.json",
    "G3": "deleted_pod_logs.json",
}
SETUP_FILE = "setup.json"
COMMIT_RE = r"[0-9a-f]{40}"
IMAGE_RE = r"ghcr\.io/faberline/sift@(sha256:[0-9a-f]{64})"


def fail(message: str) -> None:
    raise SystemExit(f"single-node receipt refused: {message}")


def load_json(path: Path, label: str) -> dict:
    if path.is_symlink() or not path.is_file():
        fail(f"{label} must be a regular file")
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError):
        fail(f"{label} is not valid JSON")
    if not isinstance(value, dict):
        fail(f"{label} must be a JSON object")
    return value


def check_gate(gate: str, name: str, record: dict) -> dict:
    if record.get("gate") != gate:
        fail(f"{gate}: {name} is not the evidence of {gate}")
    if record.get("result") != "pass":
        fail(f"{gate}: result is not pass")
    selected = record.get("selected")
    accepted = record.get("accepted")
    for field, value in (("selected", selected), ("accepted", accepted)):
        if type(value) is not int or value <= 0:
            fail(f"{gate}: {field} must be a positive integer")
    if accepted != selected:
        fail(f"{gate}: accepted {accepted} differs from selected {selected}")
    for field in ("proves", "does_not_prove"):
        text = record.get(field)
        if not isinstance(text, str) or not text.strip():
            fail(f"{gate}: {field} must be a non-empty text")
    return {
        "gate": gate,
        "phase": record.get("phase"),
        "result": "pass",
        "selected": selected,
        "accepted": accepted,
        "proves": record["proves"],
        "does_not_prove": record["does_not_prove"],
    }


def make_receipt(evidence_dir: Path, run_id: str, attempt: str, commit: str, run_commit: str) -> dict:
    if not re.fullmatch(r"[1-9][0-9]*", run_id):
        fail("run id must be a positive integer")
    if not re.fullmatch(r"[1-9][0-9]*", attempt):
        fail("attempt must be a positive integer")
    if not re.fullmatch(COMMIT_RE, commit):
        fail("commit must be 40 lowercase hex characters")
    if not re.fullmatch(COMMIT_RE, run_commit):
        fail("commit of the run must be 40 lowercase hex characters")
    if commit != run_commit:
        fail(f"commit {commit} differs from the commit of the run {run_commit}")

    setup = load_json(evidence_dir / SETUP_FILE, SETUP_FILE)
    image = setup.get("image")
    match = re.fullmatch(IMAGE_RE, image) if isinstance(image, str) else None
    if match is None:
        fail("image digest must be an immutable ghcr.io/faberline/sift digest")
    digest = match.group(1)

    gates = {}
    for gate in ("G1", "G2", "G3"):
        name = GATE_FILES[gate]
        path = evidence_dir / name
        if not path.exists() and not path.is_symlink():
            fail(f"{gate}: evidence file {name} is missing")
        gates[gate] = check_gate(gate, name, load_json(path, name))

    return {
        "schema": SCHEMA,
        "run_id": run_id,
        "attempt": attempt,
        "commit": commit,
        "image_digest": digest,
        "gates": gates,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--attempt", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run-commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    receipt = make_receipt(args.evidence_dir, args.run_id, args.attempt, args.commit, args.run_commit)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    sys.stdout.write(f"single-node receipt written: {args.output}\n")


if __name__ == "__main__":
    main()
