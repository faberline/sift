"""Resolve the current Sift PR head from a successful trusted verifier run."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile

from pr_identity import github_fetch, validate, validate_issue
from verify_online import ROOT, download_artifact, unique_object

REPOSITORY = 'faberline/sift'
REPO_API = f'/repos/{REPOSITORY}'
VERIFIER_PATH = '.github/workflows/sift-pr-verifier.yml'
WORKER_PATH = '.github/workflows/sift-pr-verify.yml'
MAX_VERDICT_ARCHIVE = 65536
MAX_VERDICT_BYTES = 4096


def positive_int(value):
    return type(value) is int and 0 < value < 2**63


def sha(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{40}', value) is not None


def checked_run(run_id, event, path):
    if not positive_int(run_id):
        raise ValueError('invalid run ID')
    run = github_fetch(f'{REPO_API}/actions/runs/{run_id}')
    if (run.get('id') != run_id or run.get('event') != event or run.get('path') != path
            or run.get('status') != 'completed' or run.get('conclusion') != 'success'
            or run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('head_repository', {}).get('full_name') != REPOSITORY
            or not sha(run.get('head_sha')) or not positive_int(run.get('run_attempt'))
            or not positive_int(run.get('workflow_id'))):
        raise ValueError('run is not a successful same-repository run of the expected workflow')
    workflow = github_fetch(f"{REPO_API}/actions/workflows/{run['workflow_id']}")
    if workflow.get('id') != run['workflow_id'] or workflow.get('path') != path:
        raise ValueError('run workflow identity differs')
    return run


def read_verdict(run):
    run_id = run['id']
    listing = github_fetch(f'{REPO_API}/actions/runs/{run_id}/artifacts?per_page=100')
    artifacts = listing.get('artifacts', [])
    if listing.get('total_count') != len(artifacts) or len(artifacts) > 100:
        raise ValueError('incomplete verifier artifact inventory')
    name = f"sift-trusted-verdict-{run_id}-{run['run_attempt']}"
    matching = [artifact for artifact in artifacts if artifact.get('name') == name]
    if len(matching) != 1:
        raise ValueError('missing or ambiguous verifier artifact for the current attempt')
    artifact = matching[0]
    size = artifact.get('size_in_bytes')
    metadata = artifact.get('workflow_run', {})
    if (artifact.get('expired') is not False or not positive_int(artifact.get('id'))
            or not positive_int(size) or size > MAX_VERDICT_ARCHIVE
            or metadata.get('id') != run_id or metadata.get('head_sha') != run['head_sha']
            or metadata.get('head_branch') != 'main'):
        raise ValueError('verifier artifact is expired, oversized or bound to a different run')
    archive = download_artifact(artifact['id'])
    if (len(archive) != size
            or artifact.get('digest') != 'sha256:' + hashlib.sha256(archive).hexdigest()):
        raise ValueError('verifier artifact digest or size differs')
    with zipfile.ZipFile(io.BytesIO(archive)) as handle:
        if handle.namelist() != ['sift-trusted-verdict.json']:
            raise ValueError('verifier artifact must contain exactly one verdict')
        info = handle.infolist()[0]
        if info.file_size > MAX_VERDICT_BYTES or info.flag_bits & 1:
            raise ValueError('verdict is oversized or encrypted')
        with handle.open(info) as stream:
            data = stream.read(MAX_VERDICT_BYTES + 1)
    if len(data) > MAX_VERDICT_BYTES:
        raise ValueError('verdict exceeds cap')
    verdict = json.loads(data, object_pairs_hook=unique_object)
    if (verdict.get('schema') != 'sift.trusted-verdict/v1'
            or verdict.get('verified') is not True or verdict.get('result') != 'PASS'
            or verdict.get('production_accepted') is not False
            or verdict.get('verifier_revision') != run['head_sha']
            or not sha(verdict.get('head_sha')) or not sha(verdict.get('base_sha'))
            or not positive_int(verdict.get('issue_number'))
            or not positive_int(verdict.get('test_run_id'))):
        raise ValueError('verdict is not a passing, revision-bound test verdict')
    return verdict


def require_ancestor(revision, default_head):
    result = subprocess.run(
        ['git', 'merge-base', '--is-ancestor', revision, default_head],
        cwd=ROOT, capture_output=True, timeout=30,
    )
    if result.returncode != 0:
        raise ValueError('verifier revision or tested base is outside trusted main history')


def resolve_candidate(verifier_run, pull_request, default_head):
    if not positive_int(pull_request) or not sha(default_head):
        raise ValueError('invalid pull request or default-branch revision')
    verifier = checked_run(verifier_run, 'workflow_run', VERIFIER_PATH)
    if verifier.get('head_branch') != 'main':
        raise ValueError('verifier did not run on main')
    verdict = read_verdict(verifier)
    require_ancestor(verdict['verifier_revision'], default_head)
    require_ancestor(verdict['base_sha'], default_head)

    worker = checked_run(verdict['test_run_id'], 'pull_request', WORKER_PATH)
    associated = worker.get('pull_requests', [])
    if (worker['head_sha'] != verdict['head_sha'] or len(associated) != 1
            or associated[0].get('number') != pull_request
            or associated[0].get('head', {}).get('sha') != verdict['head_sha']
            or associated[0].get('base', {}).get('ref') != 'main'
            or associated[0].get('base', {}).get('sha') != verdict['base_sha']):
        raise ValueError('worker run was for another pull request or tested source')
    pr = github_fetch(f'{REPO_API}/pulls/{pull_request}')
    if pr.get('number') != pull_request or pr.get('state') != 'open':
        raise ValueError('pull request is no longer open')
    identity = validate({'repository': {'full_name': REPOSITORY}, 'pull_request': pr}, verdict['head_sha'])
    if identity['issue_number'] != verdict['issue_number']:
        raise ValueError('pull request no longer names the verified issue')
    number = identity['issue_number']
    validate_issue(github_fetch(f'{REPO_API}/issues/{number}'), number)
    return identity['head_sha']


def cli_id(value):
    if re.fullmatch(r'[1-9][0-9]{0,18}', value) and positive_int(int(value)):
        return int(value)
    raise argparse.ArgumentTypeError('expected a positive numeric ID')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verifier-run', required=True, type=cli_id)
    parser.add_argument('--pull-request', required=True, type=cli_id)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if (os.environ.get('GITHUB_EVENT_NAME') != 'workflow_dispatch'
            or os.environ.get('GITHUB_REPOSITORY') != REPOSITORY
            or os.environ.get('GITHUB_REF') != 'refs/heads/main'):
        raise ValueError('candidate resolution requires a Sift workflow dispatch on main')
    default_head = os.environ.get('GITHUB_SHA')
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, timeout=30).strip()
    if checkout != default_head:
        raise ValueError('workflow checkout differs from the dispatch revision')
    candidate = resolve_candidate(args.verifier_run, args.pull_request, default_head)
    with args.output.open('a', encoding='utf-8') as output:
        output.write(f'source_sha={candidate}\n')


if __name__ == '__main__':
    main()
