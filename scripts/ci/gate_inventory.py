"""Verifier-owned v1 inventory. Never derive acceptance requirements from a receipt."""
REQUIRED_GATES = (
    'prepare', 'ci-unit-controls', 'candidate-entrypoint', 'format', 'locked-build',
    'core-build-stamp', 'full-rust-suite', 'ignored-inventory', 'all-required-ignored',
    'clippy', 'canonical-architecture', 'zero-exceptions', 'compatibility-bytes',
    'official-rw1', 'docker-named-volume', 'source-unchanged',
)
TEST_GATES = frozenset(('ci-unit-controls', 'core-build-stamp', 'full-rust-suite',
                        'all-required-ignored', 'compatibility-bytes'))


def inventory_matches(receipt):
    declared = receipt.get('required_gates')
    gates = receipt.get('gates')
    if declared != list(REQUIRED_GATES) or not isinstance(gates, list):
        return False
    names = [gate.get('gate') for gate in gates if isinstance(gate, dict)]
    return (len(names) == len(gates) == len(REQUIRED_GATES)
            and len(set(names)) == len(names) and set(names) == set(REQUIRED_GATES))


def gates_pass(receipt):
    if not inventory_matches(receipt):
        raise ValueError('receipt inventory is empty, reduced, duplicated or mismatched')
    for gate in receipt['gates']:
        if gate.get('status') != 'PASS':
            return False
        if gate['gate'] != 'source-unchanged' and (type(gate.get('exit')) is not int or gate['exit'] != 0):
            return False
        if gate['gate'] in TEST_GATES and (type(gate.get('passed_cases')) is not int or gate['passed_cases'] <= 0):
            return False
    return True
