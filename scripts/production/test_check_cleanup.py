#!/usr/bin/env python3
"""Offline tests for the production acceptance cleanup check. No paid resource."""
import unittest

import check_cleanup

class CheckCleanup(unittest.TestCase):
    def test_empty_is_clean(self):
        self.assertEqual(check_cleanup.check([], [], []), {'status': 'clean'})

    def test_nodepool_leftover(self):
        with self.assertRaisesRegex(ValueError, 'sift-prod-acc-pool'):
            check_cleanup.check([{'name': 'sift-prod-acc-pool'}], [], [])

    def test_disk_name_leftover(self):
        with self.assertRaises(ValueError):
            check_cleanup.check([], [{'name': 'pvc-sift-prod-acc-1'}], [])

    def test_disk_description_leftover(self):
        disk = {'name': 'pvc-1234', 'description': '{"kubernetes.io/created-for/pvc/namespace":"sift-prod-acc"}'}
        with self.assertRaises(ValueError):
            check_cleanup.check([], [disk], [])

    def test_other_instance_is_clean(self):
        self.assertEqual(check_cleanup.check([], [], [{'name': 'other-vm'}]), {'status': 'clean'})

    def test_non_list_is_refused(self):
        with self.assertRaises(ValueError):
            check_cleanup.check({}, [], [])
        with self.assertRaises(ValueError):
            check_cleanup.check([], [], ['x'])

if __name__ == '__main__':
    unittest.main()
