"""Pure validation for exact-head, same-repository, issue-linked Sift PRs."""
import re


def validate(event, checked_head):
    pr = event.get('pull_request', {})
    repository = event.get('repository', {}).get('full_name')
    if repository != 'faberline/sift':
        raise ValueError('repository must be faberline/sift')
    if pr.get('head', {}).get('repo', {}).get('full_name') != repository:
        raise ValueError('head must belong to the Sift repository')
    if pr.get('base', {}).get('repo', {}).get('full_name') != repository:
        raise ValueError('base must belong to the Sift repository')
    if pr.get('base', {}).get('ref') != 'main':
        raise ValueError('base branch must be main')
    head = pr.get('head', {}).get('sha', '')
    base = pr.get('base', {}).get('sha', '')
    if not re.fullmatch(r'[0-9a-f]{40}', head) or checked_head != head:
        raise ValueError('checkout must match the exact PR head')
    if not re.fullmatch(r'[0-9a-f]{40}', base):
        raise ValueError('base SHA must be complete')
    # Require an explicit repository-qualified reference, never a guessed issue.
    lines = [line for line in (pr.get('body') or '').splitlines() if line.startswith('Refs:')]
    if len(lines) != 1 or not re.fullmatch(r'Refs: faberline/sift#[1-9][0-9]*', lines[0]):
        raise ValueError('PR must name one Sift issue with Refs: faberline/sift#N')
    return {'head_sha': head, 'base_sha': base, 'issue_number': int(lines[0].rsplit('#', 1)[1])}


def validate_issue(issue, number):
    if ('pull_request' in issue or type(issue.get('number')) is not int or issue.get('number') != number
            or issue.get('repository_url') != 'https://api.github.com/repos/faberline/sift'
            or issue.get('state') != 'open'):
        raise ValueError('linked item must be an open issue in faberline/sift, not a PR')


def validate_current(event, current, checked_head):
    result = validate(event, checked_head)
    now = validate({'repository': event['repository'], 'pull_request': current}, checked_head)
    if result != now or current.get('state') != 'open':
        raise ValueError('PR head, base, issue or state changed while checks ran')
    return result


def verify_live(event, checked_head, fetch):
    result = validate(event, checked_head)
    number = event.get('number')
    if type(number) is not int or number < 1:
        raise ValueError('missing PR number')
    result = validate_current(event, fetch(f'/repos/faberline/sift/pulls/{number}'), checked_head)
    validate_issue(fetch(f"/repos/faberline/sift/issues/{result['issue_number']}"), result['issue_number'])
    return dict(result, verified=True)


def github_fetch(path):
    import json
    import os
    import urllib.request

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    if not path.startswith('/repos/faberline/sift/'):
        raise ValueError('API path is outside Sift')
    request = urllib.request.Request('https://api.github.com' + path, headers={
        'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
        'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'})
    with urllib.request.build_opener(NoRedirect).open(request, timeout=30) as response:
        data = response.read(2097153)
    if len(data) > 2097152:
        raise ValueError('API response exceeds limit')
    return json.loads(data)


if __name__ == '__main__':
    import argparse
    import json
    from pathlib import Path
    parser = argparse.ArgumentParser()
    parser.add_argument('--event', required=True)
    parser.add_argument('--head', required=True)
    parser.add_argument('--current-pr')
    parser.add_argument('--issue')
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    event = json.loads(Path(args.event).read_text())
    result = validate(event, args.head)
    result['verified'] = False
    if args.live:
        result = verify_live(event, args.head, github_fetch)
    if args.current_pr or args.issue:
        if not (args.current_pr and args.issue):
            parser.error('current PR and issue evidence must be supplied together')
        result = validate_current(event, json.loads(Path(args.current_pr).read_text()), args.head)
        validate_issue(json.loads(Path(args.issue).read_text()), result['issue_number'])
        result['verified'] = True
    Path(args.output).write_text(json.dumps(result, sort_keys=True) + '\n')
