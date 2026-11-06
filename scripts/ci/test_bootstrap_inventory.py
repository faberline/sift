import copy
import json
from pathlib import Path
import unittest

from finalize import finalize
from gate_inventory import REQUIRED_GATES, TEST_GATES
from verify_online import DRIVER_PATHS, TRUSTED_PATHS, approved_driver, verify_source_tree


class BootstrapInventoryControls(unittest.TestCase):
    def setUp(self):
        self.identity = {'head_sha': 'a' * 40, 'base_sha': 'b' * 40, 'issue_number': 8, 'verified': True}
        self.receipt = {'schema': 'sift.online-verification/v1', **self.identity,
                        'required_gates': list(REQUIRED_GATES),
                        'gates': [{'gate': name, 'status': 'PASS', 'exit': 0, 'passed_cases': 1}
                                  for name in REQUIRED_GATES]}

    def test_complete_fixed_inventory_can_pass(self):
        self.assertEqual(finalize(self.receipt, self.identity)['result'], 'PASS')

    def test_receipt_cannot_reduce_or_duplicate_the_fixed_inventory(self):
        cases = [([], []), (['format'], [{'gate': 'format', 'status': 'PASS', 'exit': 0}]),
                 (list(REQUIRED_GATES), self.receipt['gates'][:-1]),
                 (list(REQUIRED_GATES), self.receipt['gates'] + self.receipt['gates'][:1])]
        for declared, gates in cases:
            with self.subTest(declared=declared), self.assertRaises(ValueError):
                finalize(dict(self.receipt, required_gates=declared, gates=gates), self.identity)

    def test_failed_exit_and_zero_test_work_cannot_pass(self):
        for name in TEST_GATES:
            changed = copy.deepcopy(self.receipt)
            next(g for g in changed['gates'] if g['gate'] == name)['passed_cases'] = 0
            self.assertEqual(finalize(changed, self.identity)['result'], 'FAIL')
        changed = copy.deepcopy(self.receipt)
        changed['gates'][0]['exit'] = 7
        self.assertEqual(finalize(changed, self.identity)['result'], 'FAIL')

    def test_manifest_is_data_only_with_exact_driver_path_inventory(self):
        path = Path(__file__).with_name('approved-p2-driver.json')
        data = json.loads(path.read_text())
        expected = approved_driver(data)
        self.assertEqual(set(expected), set(DRIVER_PATHS))
        self.assertFalse(set(expected) & set(TRUSTED_PATHS))
        for changed in [{}, dict(data, blobs={}), dict(data, blobs={'../gate.py': 'a' * 40})]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                approved_driver(changed)

    def test_future_pr_cannot_change_approved_driver_bytes(self):
        data = json.loads(Path(__file__).with_name('approved-p2-driver.json').read_text())
        expected = approved_driver(data)
        tree = {'truncated': False, 'tree': [{'type': 'blob', 'path': path, 'sha': digest}
                                            for path, digest in expected.items()]}
        verify_source_tree(tree, expected)
        tree['tree'][0]['sha'] = '0' * 40
        with self.assertRaises(ValueError):
            verify_source_tree(tree, expected)


if __name__ == '__main__':
    unittest.main()
