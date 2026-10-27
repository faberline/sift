"""Trusted workflow_run verifier: never checkout, import or execute PR code."""
import io
import json
import os
from pathlib import Path
import subprocess
import urllib.error
import urllib.parse
import urllib.request
import zipfile

from finalize import finalize
from pr_identity import github_fetch, validate, validate_issue

ROOT = Path(__file__).resolve().parents[2]
MAX_RECEIPT = 524288
MAX_ARCHIVE = 614400
TRUSTED_PATHS = (
    '.github/workflows/sift-pr-verifier.yml', 'scripts/ci/verify_online.py',
    'scripts/ci/pr_identity.py', 'scripts/ci/finalize.py', 'scripts/ci/gate_inventory.py',
    'scripts/ci/approved-p2-driver.json', 'scripts/ci/test_pr_identity.py',
    'scripts/ci/test_verify_online.py', 'scripts/ci/test_bootstrap_inventory.py',
)
DRIVER_PATHS = (
    '.github/workflows/sift-pr-verify.yml', 'Dockerfile', 'test.sh',
    'tests/candidate_root.sh', 'tests/prometheus_compliance.sh', 'tests/docker_named_volume.sh',
    'scripts/faberline-core-test.sh', 'scripts/ci/online.py', 'scripts/ci/prepare.py',
    'scripts/ci/online-dependencies.lock.json', 'scripts/ci/checker-requirements.lock',
    'scripts/ci/test_online.py',
)


def approved_driver(data):
    if (data.get('schema') != 'sift.approved-p2-driver/v1'
            or set(data.get('blobs', {})) != set(DRIVER_PATHS)):
        raise ValueError('approved driver inventory differs from the fixed bootstrap contract')
    import re
    if not all(re.fullmatch(r'[0-9a-f]{40}', value) for value in data['blobs'].values()):
        raise ValueError('driver source must use complete Git blob hashes')
    return data['blobs']



def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def read_receipt(archive):
    if len(archive) > MAX_ARCHIVE:
        raise ValueError('artifact archive exceeds cap')
    with zipfile.ZipFile(io.BytesIO(archive)) as handle:
        if handle.namelist() != ['receipt.json']:
            raise ValueError('artifact contains unexpected paths or duplicate files')
        info = handle.infolist()[0]
        if info.file_size > MAX_RECEIPT or info.flag_bits & 1:
            raise ValueError('receipt is oversized or encrypted')
        with handle.open(info) as stream:
            data = stream.read(MAX_RECEIPT + 1)
    if len(data) > MAX_RECEIPT:
        raise ValueError('receipt exceeds cap')
    return json.loads(data, object_pairs_hook=unique_object)


def verify_run(run, workflow, current_pr, receipt):
    if (run.get('event') != 'pull_request' or run.get('conclusion') != 'success'
            or run.get('head_repository', {}).get('full_name') != 'faberline/sift'
            or workflow.get('path') != '.github/workflows/sift-pr-verify.yml'):
        raise ValueError('unexpected test workflow, source or conclusion')
    prs = run.get('pull_requests', [])
    if len(prs) != 1 or prs[0].get('number') != current_pr.get('number'):
        raise ValueError('run must be associated with exactly the current PR')
    if current_pr.get('state') != 'open':
        raise ValueError('PR is no longer open')
    identity = validate({'repository': {'full_name': 'faberline/sift'}, 'pull_request': current_pr}, run['head_sha'])
    if (str(receipt.get('run_id')) != str(run['id'])
            or str(receipt.get('run_attempt')) != str(run['run_attempt'])):
        raise ValueError('artifact belongs to another run or attempt')
    return dict(identity, verified=True)


def download_artifact(artifact_id):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    url = f'https://api.github.com/repos/faberline/sift/actions/artifacts/{artifact_id}/zip'
    request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
        'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'})
    try:
        response = urllib.request.build_opener(NoRedirect).open(request, timeout=30)
    except urllib.error.HTTPError as error:
        if error.code != 302:
            raise ValueError('artifact API refused download') from None
        location = error.headers.get('Location', '')
        parsed = urllib.parse.urlparse(location)
        host = parsed.hostname or ''
        if parsed.scheme != 'https' or not host.endswith(('.blob.core.windows.net', '.githubusercontent.com', '.s3.amazonaws.com')):
            raise ValueError('unexpected artifact download host')
        # Signed URL is used without the API Authorization header and is never logged.
        response = urllib.request.build_opener(NoRedirect).open(location, timeout=30)
    with response:
        data = response.read(MAX_ARCHIVE + 1)
    if len(data) > MAX_ARCHIVE:
        raise ValueError('artifact download exceeds cap')
    return data


def verify_source_tree(tree, expected):
    if tree.get('truncated'):
        raise ValueError('source tree response is incomplete')
    actual = {entry['path']: entry['sha'] for entry in tree.get('tree', []) if entry.get('type') == 'blob'}
    if any(actual.get(path) != digest for path, digest in expected.items()):
        raise ValueError('critical workflow/gate code differs from the trusted bootstrap; independent bootstrap review required')


def main():
    if (os.environ.get('GITHUB_EVENT_NAME') != 'workflow_run'
            or os.environ.get('GITHUB_REPOSITORY') != 'faberline/sift'
            or os.environ.get('GITHUB_REF') != 'refs/heads/main'):
        raise ValueError('verifier requires the trusted default-branch workflow_run entry')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if revision != os.environ['SIFT_VERIFIER_REVISION']:
        raise ValueError('verifier checkout differs from the fixed workflow revision')
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    run_id = event['workflow_run']['id']
    if type(run_id) is not int:
        raise ValueError('invalid workflow run identity')
    run = github_fetch(f'/repos/faberline/sift/actions/runs/{run_id}')
    prs = run.get('pull_requests', [])
    if len(prs) != 1 or type(prs[0].get('number')) is not int:
        raise ValueError('missing or ambiguous associated PR')
    pr = github_fetch(f"/repos/faberline/sift/pulls/{prs[0]['number']}")
    workflow = github_fetch(f"/repos/faberline/sift/actions/workflows/{run['workflow_id']}")
    artifacts = github_fetch(f'/repos/faberline/sift/actions/runs/{run_id}/artifacts?per_page=100')
    if artifacts.get('total_count') != len(artifacts.get('artifacts', [])) or artifacts['total_count'] > 100:
        raise ValueError('artifact inventory is incomplete or exceeds the bounded page')
    name = f"sift-pr-{pr['number']}-{run['head_sha']}-{run['run_attempt']}"
    selected = [a for a in artifacts.get('artifacts', []) if a.get('name') == name and not a.get('expired')]
    if len(selected) != 1 or selected[0].get('size_in_bytes', MAX_ARCHIVE + 1) > MAX_ARCHIVE:
        raise ValueError('missing, duplicate or oversized source-bound artifact')
    receipt = read_receipt(download_artifact(selected[0]['id']))
    identity = verify_run(run, workflow, pr, receipt)
    issue = github_fetch(f"/repos/faberline/sift/issues/{identity['issue_number']}")
    validate_issue(issue, identity['issue_number'])
    expected = {path: subprocess.check_output(['git', 'hash-object', path], cwd=ROOT, text=True).strip()
                for path in TRUSTED_PATHS}
    expected.update(approved_driver(json.loads((ROOT / 'scripts/ci/approved-p2-driver.json').read_text())))
    verify_source_tree(github_fetch(f"/repos/faberline/sift/git/trees/{identity['head_sha']}?recursive=1"), expected)
    # Receipt content remains untrusted evidence; only this fixed verifier owns acceptance.
    finalized = finalize(receipt, identity)
    summary = {'schema': 'sift.trusted-verdict/v1', **identity,
               'test_run_id': run_id, 'verifier_revision': revision,
               'result': finalized['result'], 'production_accepted': False}
    path = Path(os.environ['RUNNER_TEMP']) / 'sift-trusted-verdict.json'
    path.write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, sort_keys=True))
    return 0 if summary['result'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
