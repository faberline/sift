"""Check the Sift threat model: each trust boundary has a control and a proof.

A proof is a `path::name` token. The name must be defined in the file:
a Rust `fn`, a Python `def`, or a job id of a workflow file.
"""

import pathlib
import re
import sys

BOUNDARIES = [
    "Client to API",
    "TLS ingress",
    "Webhook receiver",
    "Operator to Kubernetes API",
    "Storage and object store",
    "Image supply chain",
    "Secrets",
]

PROOF_TOKEN = re.compile(r"`([^`\s]+::[^`\s]+)`")
SEPARATOR_CELL = re.compile(r"[-: ]*")


def section_lines(lines, heading):
    """Lines after the heading line, up to the next `## ` line. None if absent."""
    for index, line in enumerate(lines):
        if line.strip() == heading:
            body = []
            for following in lines[index + 1 :]:
                if following.startswith("## "):
                    break
                body.append(following)
            return body
    return None


def table_rows(body):
    """Rows with exactly three cells, minus the header and separator lines."""
    rows = []
    for line in body:
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        cells = [cell.strip() for cell in stripped[1:-1].split("|")]
        if len(cells) != 3:
            continue
        if cells[0] == "Boundary" or SEPARATOR_CELL.fullmatch(cells[0]):
            continue
        rows.append(cells)
    return rows


def proof_exists(root, token):
    path_text, name = token.split("::", 1)
    path = root / path_text
    if not path.is_file():
        return False
    text = path.read_text(encoding="utf-8", errors="replace")
    escaped = re.escape(name)
    if path.suffix == ".rs":
        pattern = rf"\bfn {escaped}\s*\("
    elif path.suffix == ".py":
        pattern = rf"\bdef {escaped}\s*\("
    elif path.suffix in (".yml", ".yaml"):
        pattern = rf"^  {escaped}:\s*$"
    else:
        return False
    return re.search(pattern, text, re.MULTILINE) is not None


def check(page, root):
    """Return the errors in the order found. An empty list means the page is good."""
    lines = page.splitlines()
    errors = []
    rows = table_rows(section_lines(lines, "## Trust boundaries") or [])
    by_boundary = {cells[0]: cells for cells in rows}

    for name in BOUNDARIES:
        cells = by_boundary.get(name)
        if cells is None:
            errors.append(f"missing boundary: {name}")
            continue
        _, control, proof = cells
        if not control:
            errors.append(f"no control: {name}")
        if not PROOF_TOKEN.findall(proof):
            errors.append(f"no proof: {name}")

    for cells in rows:
        for token in PROOF_TOKEN.findall(cells[2]):
            if not proof_exists(root, token):
                errors.append(f"proof not found: {token}")

    review = section_lines(lines, "## Independent security review")
    if review is None:
        errors.append("missing section: Independent security review")
    else:
        text = "\n".join(review)
        if "person" not in text or "not claimed" not in text:
            errors.append("review must say: not claimed")

    return errors


def main():
    root = pathlib.Path(__file__).resolve().parents[2]
    page = (root / "docs/security/threat-model.md").read_text(encoding="utf-8")
    errors = check(page, root)
    if errors:
        for error in errors:
            print(error)
        return 1
    print("threat model ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
