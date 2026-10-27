"""Hosted-only full verification. Local users run test_online.py, never this runner."""
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import shutil
import subprocess
import sys
import time
import urllib.request
import urllib.parse

ROOT = Path(__file__).resolve().parents[2]
HEX40 = re.compile(r'[0-9a-f]{40}')
SENSITIVE = re.compile(r'token|secret|password|authorization|bearer|cookie|credential|kubeconfig|private.key', re.I)


def hosted_root(env):
    if (env.get('GITHUB_ACTIONS') != 'true' or env.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or env.get('RUNNER_OS') != 'Linux' or env.get('RUNNER_ARCH') != 'X64'
            or env.get('GITHUB_REPOSITORY') != 'faberline/sift'):
        raise ValueError('full gates require a GitHub-hosted Linux X64 runner')
    base = Path(env['RUNNER_TEMP']).resolve(strict=True)
    return base / 'sift-online'


def load_lock(path):
    data = json.loads(path.read_text())
    if data['schema'] != 'sift.online-dependencies/v1':
        raise ValueError('unknown dependency lock')
    if set(data['repositories']) != {'workspace', 'vat', 'lumen', 'tape', 'relay', 'defer'}:
        raise ValueError('dependency repository set changed')
    if not all(HEX40.fullmatch(v) for v in data['repositories'].values()):
        raise ValueError('dependencies must use complete commit SHAs')
    checker = data['checker']
    if checker['repository'] != 'faberline/workspace' or not HEX40.fullmatch(checker['revision']):
        raise ValueError('checker source must be pinned')
    if len(checker['blobs']) != 16:
        raise ValueError('checker file inventory changed')
    for path, digest in checker['blobs'].items():
        if not path.startswith('scripts/meta/') or '..' in Path(path).parts or not HEX40.fullmatch(digest):
            raise ValueError('invalid checker path or blob')
    for version in (data['rust'], data['python'], data['uv'], data['docker']):
        if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version):
            raise ValueError('tools require exact versions')
    if not all(re.fullmatch(r'[^\s]+@sha256:[0-9a-f]{64}', image) for image in data['images'].values()):
        raise ValueError('container tools require immutable image digests')
    if sum(data['ignored_cases'].values()) != 24:
        raise ValueError('review changes to the required ignored inventory')
    return data


def verify_blob(data, expected):
    actual = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
    if actual != expected:
        raise ValueError('canonical checker blob differs')


def sanitize(data, limit=16384):
    text = data.decode('utf-8', errors='replace')
    text = re.sub(r'-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----',
                  '[redacted]', text, flags=re.S)
    text = '\n'.join('[redacted]' if SENSITIVE.search(line) else line for line in text.splitlines())
    bounded = text.encode()[-limit:]
    if len(text.encode()) > limit:
        bounded = bounded.split(b'\n', 1)[1] if b'\n' in bounded else b''
    return bounded.decode('utf-8', errors='replace')


def passed_count(data):
    return sum(int(n) for n in re.findall(rb'test result: ok\. ([0-9]+) passed;', data)
               + re.findall(rb'\nRan ([0-9]+) tests? in ', data))


def outcome(exit_code, output, require_tests=False):
    return 'PASS' if exit_code == 0 and (not require_tests or passed_count(output) > 0) else 'FAIL'


def successful(results, required):
    names = [r['gate'] for r in results]
    return (len(names) == len(set(names)) and set(names) == set(required)
            and all(r['status'] == 'PASS' for r in results))


def execute(command, cwd, env, timeout=1800, cap=16777216):
    """Bound output/time, kill the whole test process group, never stream raw logs."""
    proc = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + timeout
    data = bytearray()
    reason = None
    try:
        while selector.get_map():
            if time.monotonic() > deadline:
                reason = 'timeout'
                break
            for key, _ in selector.select(min(0.2, max(0, deadline - time.monotonic()))):
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                data.extend(chunk)
                if len(data) > cap:
                    reason = 'output-limit'
                    break
            if reason:
                break
        if reason:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        code = proc.wait(timeout=10)
    finally:
        selector.close()
        proc.stdout.close()
        # Test descendants must not survive a finished gate either.
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()
    if reason:
        data = data[:cap] + ('\nrunner: ' + reason).encode()
        code = 124 if reason == 'timeout' else 125
    return code, bytes(data)


def canonical_files(checker, destination):
    for path, expected in checker['blobs'].items():
        url = f"https://raw.githubusercontent.com/{checker['repository']}/{checker['revision']}/{path}"
        with urllib.request.urlopen(url, timeout=30) as response:
            if urllib.parse.urlparse(response.url).hostname != 'raw.githubusercontent.com':
                raise ValueError('unexpected checker redirect')
            data = response.read(1048577)
        if len(data) > 1048576:
            raise ValueError('checker file exceeds limit')
        verify_blob(data, expected)
        target = destination / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def architecture_clean(path):
    data = json.loads(path.read_text())
    return (data.get('schema') == 'rust-arch-report/1' and data.get('mode') == 'strict'
            and data.get('exit_code') == 0 and len(data.get('repos', [])) == 1
            and data['repos'][0].get('status') == 'pass'
            and not data['repos'][0].get('findings'))


def ignored_inventory(output, expected):
    found = {name: 0 for name in expected}
    total = 0
    for line in output.decode().splitlines():
        if line.endswith(': test'):
            total += 1
            name = line.split('::', 1)[0]
            if name not in found:
                return False
            found[name] += 1
    return found == expected and total == sum(expected.values())


def clean_source(head):
    actual = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    dirty = subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True)
    if actual != head or dirty:
        raise ValueError('source changed or checkout differs from the event head')


def test_environment(source_env, identity, task):
    head = identity.get('head_sha', '')
    if not HEX40.fullmatch(head):
        raise ValueError('test source identity requires a complete SHA')
    env = {k: v for k, v in source_env.items() if not (
        SENSITIVE.search(k) or k.startswith(('AWS_', 'CLOUDSDK_', 'GOOGLE_', 'AZURE_')))}
    env.update(CARGO_TARGET_DIR=str(task / 'target'), SIFT_REPO_ROOT=str(ROOT),
               SIFT_SOURCE_REVISION=head, SIFT_EXPECTED_SOURCE_REVISION=head,
               CARGO_BUILD_JOBS='2', UV_CACHE_DIR=str(task / 'uv-cache'))
    return env


def run():
    if any(key in os.environ for key in ('SIFT_BIN', 'VAT_OBSERVABILITY_PROBE', 'VAT_SERVICE_OBSERVABILITY_PROBE')):
        raise ValueError('caller binary overrides and inherited VAT child probes are refused')
    task = hosted_root(os.environ)
    task.mkdir(mode=0o700, exist_ok=False)
    evidence = task / 'evidence'
    evidence.mkdir(mode=0o700)
    lock_path = ROOT / 'scripts/ci/online-dependencies.lock.json'
    lock = load_lock(lock_path)
    identity_path = Path(os.environ['SIFT_CI_IDENTITY'])
    identity = json.loads(identity_path.read_text())
    if identity.get('verified') is not False:
        raise ValueError('test job takes event claims only; API verification belongs to the independent verifier')
    clean_source(identity['head_sha'])
    env = test_environment(os.environ, identity, task)
    results = []

    def gate(name, command, tests=False, cwd=ROOT, timeout=1800):
        started = time.monotonic()
        try:
            code, output = execute(command, cwd, env, timeout, lock['limits']['gate_output_bytes'])
            status = outcome(code, output, tests)
        except (OSError, ValueError) as exc:
            code, output, status = 126, str(exc).encode(), 'BLOCKED'
        result = {'gate': name, 'status': status, 'exit': code,
                  'command': command,
                  'seconds': round(time.monotonic() - started, 3),
                  'passed_cases': passed_count(output) if tests else None,
                  'output_sha256': hashlib.sha256(output).hexdigest(),
                  'output_bytes': len(output),
                  'tail': sanitize(output, lock['limits']['artifact_tail_bytes'])}
        results.append(result)
        print(f'{name}: {status}', flush=True)
        return status == 'PASS', output

    # Preparation failure remains a required failure while independent gates run.
    prep_ok, _ = gate('prepare', [sys.executable, str(ROOT / 'scripts/ci/prepare.py'), str(task)], timeout=3600)
    env['PATH'] = str(task / 'bin') + os.pathsep + env['PATH']
    env['FABERLINE_WORKSPACE'] = str(task / 'sources/workspace')
    gate('ci-unit-controls', [sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'scripts/ci', '-p', 'test_*.py', '-v'], tests=True, timeout=120)
    gate('candidate-entrypoint', ['bash', 'tests/candidate_root.sh'], timeout=60)
    gate('format', ['cargo', 'fmt', '--package', 'sift', '--', '--check'], timeout=120)
    build_ok, _ = gate('locked-build', ['cargo', 'build', '--locked', '-p', 'sift', '--bins'])
    gate('core-build-stamp', ['bash', 'scripts/faberline-core-test.sh', 'build-stamp', '--lib'], tests=True)
    gate('full-rust-suite', ['cargo', 'test', '--locked', '-p', 'sift'], tests=True)
    inventory_ok, inventory = gate('ignored-inventory', ['cargo', 'test', '--locked', '-p', 'sift', '--test', 'it', '--', '--ignored', '--list'])
    if inventory_ok and not ignored_inventory(inventory, lock['ignored_cases']):
        results[-1]['status'] = 'FAIL'
        results[-1]['tail'] = 'Required ignored-test inventory differs; review additions/removals.'
    if prep_ok:
        gate('all-required-ignored', ['cargo', 'test', '--locked', '-p', 'sift', '--test', 'it', '--', '--ignored', '--test-threads=1'], tests=True)
    else:
        results.append({'gate': 'all-required-ignored', 'status': 'BLOCKED', 'reason': 'pinned environment preparation failed'})
    gate('clippy', ['cargo', 'clippy', '--locked', '-p', 'sift', '--all-targets', '--', '-D', 'warnings'])
    arch_json = task / 'architecture.json'
    arch_ok, _ = gate('canonical-architecture', [str(task / 'checker-env/bin/python'), str(task / 'checker/scripts/meta/rust_arch_contract.py'),
        'check', '--repo', str(ROOT), '--strict', '--base', identity['base_sha'], '--json', str(arch_json)], timeout=180)
    if arch_ok and not architecture_clean(arch_json):
        results[-1]['status'] = 'FAIL'
        results[-1]['tail'] = 'Architecture has findings, warnings or suppressed exceptions.'
    # A separate subprocess source audit: no parsing/rewriting or severity override.
    gate('zero-exceptions', [sys.executable, '-c',
        "import pathlib,re,sys; t=pathlib.Path('ddd.toml').read_text(); sys.exit(bool(re.search(r'^\\[\\[(exceptions|renames)\\]\\]',t,re.M)))"], timeout=30)
    if build_ok:
        gate('compatibility-bytes', ['cargo', 'test', '--locked', '-p', 'sift', '--test', 'it', '--', 'refactor_compatibility::'], tests=True)
        gate('official-rw1', ['bash', 'tests/prometheus_compliance.sh'])
        # All Cargo/compatibility/RW gates are finished. Docker builds its own
        # source image, so these task-owned intermediates are no longer needed.
        shutil.rmtree(task / 'target', ignore_errors=False)
        gate('docker-named-volume', ['bash', 'tests/docker_named_volume.sh'])
    else:
        for name in ('compatibility-bytes', 'official-rw1', 'docker-named-volume'):
            results.append({'gate': name, 'status': 'BLOCKED', 'reason': 'Sift build failed'})
    try:
        clean_source(identity['head_sha'])
        results.append({'gate': 'source-unchanged', 'status': 'PASS'})
    except ValueError:
        results.append({'gate': 'source-unchanged', 'status': 'FAIL'})
    success = successful(results, lock['required_gates'])
    tools = task / 'tool-identities.json'
    receipt = {'schema': 'sift.online-verification/v1', **identity,
        'dependency_lock_sha256': hashlib.sha256(lock_path.read_bytes()).hexdigest(),
        'run_id': os.environ['GITHUB_RUN_ID'], 'run_attempt': os.environ['GITHUB_RUN_ATTEMPT'],
        'runner_image': os.environ.get('ImageVersion', 'unknown'),
        'result': 'UNVERIFIED_TEST_REPORT' if success else 'FAIL', 'gates': results,
        'required_gates': lock['required_gates'], 'identity_after_verified': False,
        'tool_identities': json.loads(tools.read_text()) if tools.is_file() else {},
        'gate_source_sha256': {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in [Path(__file__), ROOT / 'scripts/ci/prepare.py', ROOT / 'test.sh', ROOT / 'tests/candidate_root.sh']},
        'production_accepted': False}
    encoded = (json.dumps(receipt, indent=2) + '\n').encode()
    if len(encoded) > lock['limits']['artifact_total_bytes']:
        raise ValueError('evidence exceeds artifact cap')
    (evidence / 'receipt.json').write_bytes(encoded)
    return 0 if success else 1


if __name__ == '__main__':
    raise SystemExit(run())
