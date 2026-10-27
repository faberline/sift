"""A receipt can pass only after a fresh GitHub PR/issue identity check."""
import json
from pathlib import Path
import sys

from gate_inventory import gates_pass


def finalize(receipt, identity):
    if receipt.get('schema') != 'sift.online-verification/v1' or identity.get('verified') is not True:
        raise ValueError('missing verified final identity')
    if any(receipt.get(k) != identity.get(k) for k in ('head_sha', 'base_sha', 'issue_number')):
        raise ValueError('final identity differs from gate identity')
    passed = gates_pass(receipt)
    receipt['identity_after_verified'] = True
    receipt['result'] = 'PASS' if passed else 'FAIL'
    return receipt


if __name__ == '__main__':
    path, identity_path = map(Path, sys.argv[1:])
    data = finalize(json.loads(path.read_text()), json.loads(identity_path.read_text()))
    encoded = (json.dumps(data, indent=2) + '\n').encode()
    if len(encoded) > 524288:
        raise ValueError('receipt exceeds artifact cap')
    path.write_bytes(encoded)
