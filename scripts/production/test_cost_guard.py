#!/usr/bin/env python3
"""Offline tests for the production acceptance cost guard. No paid resource."""
import unittest
from pathlib import Path

import cost_guard

WORKFLOW = Path(__file__).resolve().parents[2] / '.github/workflows/sift-production-acceptance.yml'

class CostGuard(unittest.TestCase):
    def test_under_cap_passes(self):
        self.assertIsNone(cost_guard.check(10, 0, 1, 9))

    def test_projection_over_cap_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'cost cap exceeded'):
            cost_guard.check(10, 0, 1, 11)

    def test_spent_counts(self):
        with self.assertRaises(ValueError):
            cost_guard.check(10, 9.5, 1, 1)

    def test_bad_input_is_refused(self):
        for args in [(10, 0, float('nan'), 1), (10, -1, 1, 1), (True, 0, 1, 1), (10, 0, 1, float('inf'))]:
            with self.subTest(args=args), self.assertRaises(ValueError):
                cost_guard.check(*args)

    def test_projected_cost(self):
        self.assertEqual(cost_guard.projected_cost(2, 0.5, 4), 4.0)

    def test_workflow_text(self):
        text = WORKFLOW.read_text(encoding='utf-8')
        for needle in ['workflow_dispatch', 'cost_guard.py', 'check_cleanup.py', 'if: always()',
                       'permissions: {}', 'GCP sign-in is not set up']:
            with self.subTest(needle=needle):
                self.assertIn(needle, text)
        self.assertNotIn('pull_request', text)
        self.assertNotIn('push:', text)
        lines = text.splitlines()
        for line in lines:
            if 'uses:' in line:
                self.assertRegex(line, r'@[0-9a-f]{40}')
        idx = next(i for i, line in enumerate(lines) if line.strip().startswith('dry_run:'))
        block = []
        for line in lines[idx + 1:]:
            if not line.strip():
                break
            block.append(line)
        self.assertIn('default: true', '\n'.join(block))

if __name__ == '__main__':
    unittest.main()
