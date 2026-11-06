import hashlib
import io
import json
import unittest
from unittest.mock import patch
import zipfile

import gke_candidate as candidate


class CandidateTests(unittest.TestCase):
    head = 'a' * 40
    tested_base = 'b' * 40
    main = 'c' * 40
    repo = {'full_name': candidate.REPOSITORY}

    def setUp(self):
        self.verifier = self.run_metadata(1, 10, 'workflow_run', candidate.VERIFIER_PATH, self.tested_base)
        self.verifier['head_branch'] = 'main'
        self.worker = self.run_metadata(2, 11, 'pull_request', candidate.WORKER_PATH, self.head)
        self.worker['pull_requests'] = [{
            'number': 29,
            'head': {'sha': self.head},
            'base': {'ref': 'main', 'sha': self.tested_base},
        }]
        self.pr = {
            'number': 29, 'state': 'open', 'body': 'Refs: faberline/sift#28',
            'head': {'sha': self.head, 'repo': self.repo},
            'base': {'sha': self.main, 'ref': 'main', 'repo': self.repo},
        }
        self.verdict = {
            'schema': 'sift.trusted-verdict/v1',
            'head_sha': self.head, 'base_sha': self.tested_base, 'issue_number': 28,
            'verified': True, 'result': 'PASS', 'production_accepted': False,
            'test_run_id': 2, 'verifier_revision': self.tested_base,
        }
        self.artifact = {
            'id': 3, 'name': 'sift-trusted-verdict-1-1', 'expired': False,
            'workflow_run': {'id': 1, 'head_sha': self.tested_base, 'head_branch': 'main'},
        }
        self.responses = {
            '/actions/runs/1': self.verifier,
            '/actions/runs/2': self.worker,
            '/actions/workflows/10': {'id': 10, 'path': candidate.VERIFIER_PATH},
            '/actions/workflows/11': {'id': 11, 'path': candidate.WORKER_PATH},
            '/actions/runs/1/artifacts?per_page=100': {'total_count': 1, 'artifacts': [self.artifact]},
            '/pulls/29': self.pr,
            '/issues/28': {'number': 28, 'state': 'open',
                           'repository_url': 'https://api.github.com/repos/faberline/sift'},
        }

    def run_metadata(self, run_id, workflow_id, event, path, sha):
        return {
            'id': run_id, 'workflow_id': workflow_id, 'event': event, 'path': path,
            'status': 'completed', 'conclusion': 'success', 'run_attempt': 1,
            'repository': self.repo, 'head_repository': self.repo, 'head_sha': sha,
        }

    def resolve(self, member='sift-trusted-verdict.json', tamper=False):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, 'w', compression=zipfile.ZIP_DEFLATED) as handle:
            handle.writestr(member, json.dumps(self.verdict))
        archive = buffer.getvalue()
        self.artifact['size_in_bytes'] = len(archive)
        self.artifact['digest'] = 'sha256:' + hashlib.sha256(archive).hexdigest()
        if tamper:
            archive = archive[:-1] + bytes([archive[-1] ^ 1])
        with (patch.object(candidate, 'github_fetch', side_effect=lambda path: self.responses[
                  path.removeprefix(candidate.REPO_API)]),
              patch.object(candidate, 'download_artifact', return_value=archive),
              patch.object(candidate, 'require_ancestor') as require_ancestor):
            resolved = candidate.resolve_candidate(1, 29, self.main)
            require_ancestor.assert_called_with(self.tested_base, self.main)
            return resolved

    def test_passes_when_main_has_advanced(self):
        self.assertEqual(self.resolve(), self.head)

    def test_rejects_tampered_archive(self):
        with self.assertRaisesRegex(ValueError, 'digest'):
            self.resolve(tamper=True)

    def test_rejects_archive_with_wrong_member(self):
        with self.assertRaisesRegex(ValueError, 'exactly one verdict'):
            self.resolve(member='other.json')

    def test_rejects_previous_attempt_artifact(self):
        self.verifier['run_attempt'] = 2
        with self.assertRaisesRegex(ValueError, 'current attempt'):
            self.resolve()

    def test_rejects_failed_or_fork_verifier(self):
        for field, value in [('conclusion', 'failure'), ('head_repository', {'full_name': 'elsewhere/sift'})]:
            with self.subTest(field=field), patch.dict(self.verifier, {field: value}):
                with self.assertRaisesRegex(ValueError, 'successful same-repository'):
                    self.resolve()

    def test_rejects_verdict_from_another_revision(self):
        self.verdict['verifier_revision'] = self.main
        with self.assertRaisesRegex(ValueError, 'revision-bound'):
            self.resolve()

    def test_rejects_worker_with_wrong_base(self):
        self.worker['pull_requests'][0]['base']['sha'] = self.main
        with self.assertRaisesRegex(ValueError, 'tested source'):
            self.resolve()

    def test_rejects_pull_request_with_new_head(self):
        self.pr['head']['sha'] = 'd' * 40
        with self.assertRaisesRegex(ValueError, 'exact PR head'):
            self.resolve()

    def test_rejects_closed_issue(self):
        self.responses['/issues/28']['state'] = 'closed'
        with self.assertRaisesRegex(ValueError, 'open issue'):
            self.resolve()


if __name__ == '__main__':
    unittest.main()
