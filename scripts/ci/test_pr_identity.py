import copy
import unittest
from pr_identity import validate, validate_current, validate_issue, verify_live


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.sha = 'a' * 40
        repo = {'full_name': 'faberline/sift'}
        self.event = {'repository': repo, 'pull_request': {
            'head': {'sha': self.sha, 'repo': repo},
            'base': {'ref': 'main', 'sha': 'b' * 40, 'repo': repo},
            'body': 'P2 verification\nRefs: faberline/sift#8\n'}}

    def test_exact_head_and_scoped_issue(self):
        self.assertEqual(validate(self.event, self.sha)['issue_number'], 8)

    def test_merge_ref_or_stale_checkout_is_rejected(self):
        with self.assertRaises(ValueError):
            validate(self.event, 'c' * 40)

    def test_short_sha_is_rejected(self):
        self.event['pull_request']['head']['sha'] = 'a' * 7
        with self.assertRaises(ValueError):
            validate(self.event, 'a' * 7)

    def test_fork_head_is_rejected(self):
        self.event['pull_request']['head']['repo'] = {'full_name': 'someone/sift'}
        with self.assertRaises(ValueError):
            validate(self.event, self.sha)

    def test_other_base_is_rejected(self):
        self.event['pull_request']['base']['ref'] = 'release'
        with self.assertRaises(ValueError):
            validate(self.event, self.sha)

    def test_wrong_repository_is_rejected(self):
        self.event['repository'] = {'full_name': 'faberline/lumen'}
        with self.assertRaises(ValueError):
            validate(self.event, self.sha)

    def test_missing_foreign_and_multiple_issue_links_are_rejected(self):
        for body in ['', 'Refs: faberline/lumen#8',
                     'Refs: faberline/sift#8\nRefs: faberline/sift#9']:
            with self.subTest(body=body):
                event = copy.deepcopy(self.event)
                event['pull_request']['body'] = body
                with self.assertRaises(ValueError):
                    validate(event, self.sha)

    def test_issue_zero_and_text_substring_are_rejected(self):
        for body in ['Refs: faberline/sift#0', 'quoted Refs: faberline/sift#8']:
            self.event['pull_request']['body'] = body
            with self.assertRaises(ValueError):
                validate(self.event, self.sha)

    def test_foreign_reference_alongside_local_link_is_rejected(self):
        self.event['pull_request']['body'] += 'Refs: faberline/lumen#8\n'
        with self.assertRaises(ValueError):
            validate(self.event, self.sha)

    def test_pr_in_issue_slot_and_wrong_repository_are_rejected(self):
        issue = {'number': 8, 'state': 'open', 'repository_url': 'https://api.github.com/repos/faberline/sift'}
        validate_issue(issue, 8)
        for changes in [{'pull_request': {}}, {'number': 9}, {'state': 'closed'},
                        {'repository_url': 'https://api.github.com/repos/faberline/lumen'}]:
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                validate_issue(dict(issue, **changes), 8)

    def test_changed_head_base_and_issue_are_rejected(self):
        current = copy.deepcopy(self.event['pull_request'])
        current['state'] = 'open'
        validate_current(self.event, current, self.sha)
        for field, value in [('head', {'sha': 'c' * 40, 'repo': {'full_name': 'faberline/sift'}}),
                             ('base', {'ref': 'main', 'sha': 'c' * 40, 'repo': {'full_name': 'faberline/sift'}}),
                             ('body', 'Refs: faberline/sift#9'), ('state', 'closed')]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_current(self.event, dict(current, **{field: value}), self.sha)

    def test_live_identity_uses_only_exact_sift_pr_and_issue(self):
        self.event['number'] = 42
        current = dict(self.event['pull_request'], state='open')
        issue = {'number': 8, 'state': 'open', 'repository_url': 'https://api.github.com/repos/faberline/sift'}
        responses = {'/repos/faberline/sift/pulls/42': current, '/repos/faberline/sift/issues/8': issue}
        self.assertTrue(verify_live(self.event, self.sha, responses.__getitem__)['verified'])
        responses['/repos/faberline/sift/issues/8'] = dict(issue, pull_request={})
        with self.assertRaises(ValueError):
            verify_live(self.event, self.sha, responses.__getitem__)


if __name__ == '__main__':
    unittest.main()
