#!/usr/bin/env python3
"""Check that docs/operations/configuration.md lists every CLI flag,
SIFT_* environment variable and Sift CRD field, with the right default.

Usage: verify-configuration-doc.py [--sift-bin PATH] [--page PATH]
                                   [--crd PATH] [--root PATH]

Exit codes: 0 pass, 1 a message was found, 2 a file cannot be read.
"""

import argparse
import pathlib
import re
import subprocess
import sys

import yaml

ENV_RE = re.compile(r"\bSIFT_[A-Z0-9]+(?:_[A-Z0-9]+)*\b")
# Names the code builds with format!, so the regex above cannot see them.
PEER_TLS_NAMES = {
    "SIFT_PEER_TLS_CERT",
    "SIFT_PEER_TLS_KEY",
    "SIFT_PEER_TLS_CA",
    "SIFT_PEER_MTLS",
}
COMMAND_RE = re.compile(r"^\s{2}([a-z][a-z0-9-]*)\s")
OPTION_RE = re.compile(r"^\s+(?:-[A-Za-z], )?(--[a-z0-9][a-z0-9-]*)")
DEFAULT_RE = re.compile(r"\[default: ([^\]]+)\]")
SKIP_FLAGS = {"--help", "--version"}


def _walk(node, path, out):
    out[path] = str(node["default"]) if "default" in node else None
    for name, child in (node.get("properties") or {}).items():
        _walk(child, f"{path}.{name}", out)


def crd_paths(crd):
    """Map each dotted spec./status. field path to its CRD default, or None."""
    out = {}
    versions = (crd.get("spec") or {}).get("versions") or [{}]
    schema = (versions[0].get("schema") or {}).get("openAPIV3Schema") or {}
    props = schema.get("properties") or {}
    for top in ("spec", "status"):
        for name, child in ((props.get(top) or {}).get("properties") or {}).items():
            _walk(child, f"{top}.{name}", out)
    return out


def env_vars(root):
    """Return every SIFT_* name that the product code under root/src uses."""
    names = set()
    src = root / "src"
    if src.is_dir():
        for path in src.rglob("*.rs"):
            rel = path.relative_to(src)
            if "tests" in rel.name or "tests" in rel.parts[:-1]:
                continue
            names.update(ENV_RE.findall(path.read_text(encoding="utf-8")))
    names.discard("SIFT_PEER")
    names.update(PEER_TLS_NAMES)
    return names


def parse_help(text):
    """Return (subcommand names, {flag: default or None}) from clap --help text."""
    subs = []
    flags = {}
    blocks = {}
    section = None
    current = None
    for line in text.splitlines():
        if line and not line[0].isspace():
            section = line.strip()
            current = None
            continue
        if not line.strip():
            current = None
            continue
        if section == "Commands:":
            m = COMMAND_RE.match(line)
            if m and m.group(1) != "help":
                subs.append(m.group(1))
        elif section == "Options:":
            m = OPTION_RE.match(line)
            if m:
                current = m.group(1)
                blocks[current] = [line]
            elif current is not None:
                blocks[current].append(line)
    for flag, lines in blocks.items():
        if flag in SKIP_FLAGS:
            continue
        m = DEFAULT_RE.search("\n".join(lines))
        flags[flag] = m.group(1) if m else None
    return subs, flags


def command_tree(sift_bin, path=()):
    """Return {'sift ...': {flag: default}} for the command and its subcommands."""
    result = subprocess.run(
        [sift_bin, *path, "--help"],
        capture_output=True,
        text=True,
        check=True,
    )
    subs, flags = parse_help(result.stdout)
    key = " ".join(["sift", *path])
    tree = {key: flags}
    for sub in subs:
        tree.update(command_tree(sift_bin, (*path, sub)))
    return tree


def sections(page):
    """Split the page at '### ' headings. Key: text of the first backticks."""
    out = {}
    key = None
    body = []
    for line in page.splitlines():
        if line.startswith("## ") or line.startswith("### "):
            if key is not None:
                out[key] = "\n".join(body)
            key = None
            body = []
            if line.startswith("### "):
                ticked = re.search(r"`([^`]*)`", line)
                key = ticked.group(1) if ticked else line[4:].strip()
            continue
        if key is not None:
            body.append(line)
    if key is not None:
        out[key] = "\n".join(body)
    return out


def check(page, crd, env, tree):
    """Return one message per fault. An empty list means pass."""
    messages = []
    lines = page.splitlines()

    for path, default in crd_paths(crd).items():
        hits = [line for line in lines if f"`{path}`" in line]
        if not hits:
            messages.append(f"missing CRD field {path}")
        elif default is not None and not any(default in line for line in hits):
            messages.append(f"wrong default for {path} (CRD: {default})")

    for name in sorted(env):
        if f"`{name}`" not in page:
            messages.append(f"missing environment variable {name}")

    if tree is not None:
        found = sections(page)
        for key, flags in tree.items():
            if key not in found:
                messages.append(f"missing command section {key}")
                continue
            body = found[key].splitlines()
            for flag, default in flags.items():
                hits = [line for line in body if f"`{flag}`" in line]
                if not hits:
                    messages.append(f"missing flag {flag} in {key}")
                elif default is not None and not any(default in h for h in hits):
                    messages.append(
                        f"wrong default for {flag} in {key} (help: {default})"
                    )
    return messages


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--sift-bin", help="sift binary; without it flags are not checked")
    parser.add_argument("--page", help="configuration page")
    parser.add_argument("--crd", help="Sift CRD file")
    parser.add_argument("--root", help="repository root")
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root) if args.root else pathlib.Path(__file__).resolve().parent.parent
    page_path = pathlib.Path(args.page) if args.page else root / "docs/operations/configuration.md"
    crd_path = pathlib.Path(args.crd) if args.crd else root / "k8s/crd/sift.yaml"

    try:
        page = page_path.read_text(encoding="utf-8")
        crd = yaml.safe_load(crd_path.read_text(encoding="utf-8")) or {}
    except (OSError, yaml.YAMLError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    env = env_vars(root)
    tree = None
    if args.sift_bin:
        tree = command_tree(args.sift_bin)
    else:
        print("command flags not checked")

    messages = check(page, crd, env, tree)
    if messages:
        for message in messages:
            print(message, file=sys.stderr)
        return 1
    print("configuration page ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
