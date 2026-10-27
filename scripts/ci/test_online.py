import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

from online import (ROOT, architecture_clean, execute, hosted_root, ignored_inventory,
                    load_lock, outcome, sanitize, successful, test_environment, verify_blob)
from finalize import finalize
from gate_inventory import REQUIRED_GATES, TEST_GATES


class RunnerControls(unittest.TestCase):
    def test_local_or_foreign_environment_is_refused(self):
        with self.assertRaises(ValueError):
            hosted_root({})
        with self.assertRaises(ValueError):
            hosted_root({'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
                         'RUNNER_OS': 'Linux', 'RUNNER_ARCH': 'X64', 'GITHUB_REPOSITORY': 'other/repo'})

    def test_actual_lock_and_mutable_dependency_rejection(self):
        path = ROOT / 'scripts/ci/online-dependencies.lock.json'
        data = load_lock(path)
        with tempfile.TemporaryDirectory() as root:
            target = Path(root) / 'lock.json'
            data['repositories']['vat'] = 'main'
            target.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                load_lock(target)

    def test_checker_path_escape_is_rejected(self):
        data = json.loads((ROOT / 'scripts/ci/online-dependencies.lock.json').read_text())
        key, value = data['checker']['blobs'].popitem()
        data['checker']['blobs']['scripts/meta/../../bad.py'] = value
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'lock.json'
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                load_lock(path)

    def test_checker_bytes_are_authenticated(self):
        data = b'known source\n'
        digest = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        verify_blob(data, digest)
        with self.assertRaises(ValueError):
            verify_blob(data + b'changed', digest)

    def test_zero_work_and_failed_exit_cannot_pass(self):
        self.assertEqual(outcome(0, b'test result: ok. 0 passed;', True), 'FAIL')
        self.assertEqual(outcome(1, b'test result: ok. 5 passed;', True), 'FAIL')
        self.assertEqual(outcome(0, b'test result: ok. 5 passed;', True), 'PASS')

    def test_subprocess_exit_is_retained(self):
        code, output = execute([sys.executable, '-c', 'print("control"); raise SystemExit(7)'],
                               ROOT, dict(os.environ), timeout=2)
        self.assertEqual(code, 7)
        self.assertIn(b'control', output)

    def test_output_and_time_are_bounded(self):
        code, output = execute([sys.executable, '-c', 'print("x" * 10000)'],
                               ROOT, dict(os.environ), timeout=2, cap=128)
        self.assertEqual(code, 125)
        self.assertLess(len(output), 200)
        code, _ = execute([sys.executable, '-c', 'import time; time.sleep(5)'],
                          ROOT, dict(os.environ), timeout=0.05)
        self.assertEqual(code, 124)

    def test_logs_are_sanitized_before_tail_truncation(self):
        self.assertNotIn('hidden', sanitize(b'normal\ntoken=hidden\nend'))
        self.assertNotIn('base64data', sanitize(b'-----BEGIN PRIVATE KEY-----\nbase64data\n-----END PRIVATE KEY-----'))
        self.assertEqual(sanitize(b'token=' + b'z' * 10000, 32), '[redacted]')
        self.assertEqual(sanitize(b'ordinary' * 1000, 32), '')

    def test_missing_or_new_ignored_cases_fail_inventory(self):
        expected = {'module': 1}
        self.assertTrue(ignored_inventory(b'module::case: test\n', expected))
        self.assertFalse(ignored_inventory(b'', expected))
        self.assertFalse(ignored_inventory(b'module::case: test\nother::case: test\n', expected))

    def test_architecture_warnings_and_suppression_cannot_pass(self):
        data = {'schema': 'rust-arch-report/1', 'mode': 'strict', 'exit_code': 0,
                'repos': [{'status': 'pass', 'findings': []}]}
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'report.json'
            path.write_text(json.dumps(data))
            self.assertTrue(architecture_clean(path))
            for severity in ['warning', 'error']:
                changed = copy.deepcopy(data)
                changed['repos'][0]['findings'] = [{'severity': severity, 'suppressed_by': 'exceptions[0]'}]
                path.write_text(json.dumps(changed))
                self.assertFalse(architecture_clean(path))

    def test_missing_duplicate_or_blocked_gate_cannot_pass(self):
        good = [{'gate': 'a', 'status': 'PASS'}, {'gate': 'b', 'status': 'PASS'}]
        self.assertTrue(successful(good, ['a', 'b']))
        self.assertFalse(successful(good[:1], ['a', 'b']))
        self.assertFalse(successful(good + good[:1], ['a', 'b']))
        self.assertFalse(successful([{'gate': 'a', 'status': 'BLOCKED'}], ['a']))

    def test_receipt_requires_unchanged_verified_final_identity(self):
        identity = {'head_sha': 'a' * 40, 'base_sha': 'b' * 40, 'issue_number': 8, 'verified': True}
        receipt = {'schema': 'sift.online-verification/v1', **identity,
                   'gates': [{'gate': name, 'status': 'PASS', 'exit': 0, 'passed_cases': 1}
                             for name in REQUIRED_GATES], 'required_gates': list(REQUIRED_GATES)}
        self.assertEqual(finalize(copy.deepcopy(receipt), identity)['result'], 'PASS')
        for change in [{'verified': False}, {'head_sha': 'c' * 40}, {'issue_number': 9}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                finalize(copy.deepcopy(receipt), dict(identity, **change))

    def test_receipt_cannot_choose_its_own_acceptance_inventory(self):
        identity = {'head_sha': 'a' * 40, 'base_sha': 'b' * 40, 'issue_number': 8, 'verified': True}
        full = [{'gate': name, 'status': 'PASS', 'exit': 0, 'passed_cases': 1} for name in REQUIRED_GATES]
        cases = [([], []), (['format'], [{'gate': 'format', 'status': 'PASS', 'exit': 0}]),
                 (list(REQUIRED_GATES), full[:-1]), (list(REQUIRED_GATES), full + full[:1]),
                 (list(REQUIRED_GATES) + ['extra'], full)]
        for declared, gates in cases:
            with self.subTest(declared=declared), self.assertRaises(ValueError):
                finalize({'schema': 'sift.online-verification/v1', **identity,
                          'required_gates': declared, 'gates': gates}, identity)

    def test_binary_expected_revision_is_forced_from_source_identity(self):
        head = 'a' * 40
        env = test_environment({'GH_TOKEN': 'not-forwarded', 'SIFT_EXPECTED_SOURCE_REVISION': 'b' * 40},
                               {'head_sha': head}, Path('/tmp/unit-task'))
        self.assertEqual(env['SIFT_EXPECTED_SOURCE_REVISION'], head)
        self.assertEqual(env['SIFT_SOURCE_REVISION'], head)
        self.assertNotIn('GH_TOKEN', env)


if __name__ == '__main__':
    unittest.main()
