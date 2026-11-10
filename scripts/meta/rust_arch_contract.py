#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["tree-sitter>=0.23", "tree-sitter-rust>=0.23"]
# ///
"""Check Rust repos against the structure, DDD, and large-file contract.

Each repo's ddd.toml declares its contexts, layers, and allowlists; the
organisation-wide constants live in rust_arch/policy.toml. Run with
``uv run scripts/meta/rust_arch_contract.py check --help``.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from rust_arch.cli import main

if __name__ == "__main__":
    raise SystemExit(main())
