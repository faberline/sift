import copy
import io
import json
import unittest
import zipfile

from verify_online import read_receipt, verify_run, verify_source_tree


def archive(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as handle:
        for name, data in files:
            handle.writestr(name, data)
    return output.getvalue()


class IndependentVerifierControls(unittest.TestCase):
    def setUp(self):
        repo = {'full_name': 'faberline/sift'}
        self.pr = {'number': 42, 'state': 'open', 'head': {'sha': 'a' * 40, 'repo': repo},
                   'base': {'ref': 'main', 'sha': 'b' * 40, 'repo': repo}, 'body': 'Refs: faberline/sift#8'}
        self.run = {'id': 12, 'run_attempt': 1, 'event': 'pull_request', 'conclusion': 'success',
                    'head_sha': 'a' * 40, 'head_repository': repo, 'pull_requests': [{'number': 42}]}
        self.workflow = {'path': '.github/workflows/sift-pr-verify.yml'}
        self.receipt = {'run_id': '12', 'run_attempt': '1'}

    def test_platform_run_identity_and_current_pr_are_bound(self):
        self.assertTrue(verify_run(self.run, self.workflow, self.pr, self.receipt)['verified'])
        for field, value in [('run_attempt', 2), ('event', 'workflow_dispatch'),
                             ('conclusion', 'failure'), ('head_sha', 'c' * 40), ('pull_requests', [])]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_run(dict(self.run, **{field: value}), self.workflow, self.pr, self.receipt)

    def test_unexpected_workflow_and_foreign_head_are_rejected(self):
        with self.assertRaises(ValueError):
            verify_run(self.run, {'path': 'other.yml'}, self.pr, self.receipt)
        run = dict(self.run, head_repository={'full_name': 'someone/sift'})
        with self.assertRaises(ValueError):
            verify_run(run, self.workflow, self.pr, self.receipt)

    def test_only_one_bounded_json_receipt_is_read_without_extraction(self):
        self.assertEqual(read_receipt(archive([('receipt.json', '{}')])), {})
        for files in [[('../receipt.json', '{}')], [('receipt.json', '{}'), ('extra.py', 'bad')],
                      [('receipt.json', '{"gates":[],"gates":[]}')], [('receipt.json', 'x' * 524289)]]:
            with self.subTest(files=[name for name, _ in files]), self.assertRaises(ValueError):
                read_receipt(archive(files))

    def test_changed_critical_code_or_truncated_tree_needs_bootstrap_review(self):
        tree = {'truncated': False, 'tree': [{'type': 'blob', 'path': 'gate.py', 'sha': 'a' * 40}]}
        verify_source_tree(tree, {'gate.py': 'a' * 40})
        with self.assertRaises(ValueError):
            verify_source_tree(tree, {'gate.py': 'b' * 40})
        with self.assertRaises(ValueError):
            verify_source_tree(dict(tree, truncated=True), {'gate.py': 'a' * 40})


if __name__ == '__main__':
    unittest.main()
