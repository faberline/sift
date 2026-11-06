"""Install/build only the reviewed pinned public test dependencies, on CI."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys

from online import ROOT, canonical_files, execute, hosted_root, load_lock, sanitize


def prepare(task):
    if task != hosted_root(os.environ):
        raise ValueError('preparation is restricted to the hosted task directory')
    lock = load_lock(ROOT / 'scripts/ci/online-dependencies.lock.json')
    dockerfile = (ROOT / 'Dockerfile').read_text()
    if not all(image in dockerfile for name, image in lock['images'].items() if name != 'go'):
        raise ValueError('Dockerfile differs from the locked image inputs')
    if lock['images']['go'] not in (ROOT / 'tests/prometheus_compliance.sh').read_text():
        raise ValueError('compliance tool image differs from lock')
    env = dict(os.environ)
    env['CARGO_TARGET_DIR'] = str(task / 'target-tools')
    env['PYTHONDONTWRITEBYTECODE'] = '1'

    def checked(command, cwd=ROOT, timeout=1800):
        code, output = execute(command, cwd, env, timeout)
        if code:
            print(sanitize(output), file=sys.stderr)
            raise ValueError(f'preparation command failed with exit {code}: {command[0]}')
        return output.decode().strip()

    for tool, expected in [('cargo', lock['rust']), ('rustc', lock['rust']), ('uv', lock['uv'])]:
        version = checked([tool, '--version'])
        if version.split()[1] != expected:
            raise ValueError(f'{tool} must be version {expected}')
    docker = checked(['docker', 'version', '--format', '{{.Server.Version}}'])
    if docker != lock['docker']:
        raise ValueError('Docker server version differs from lock')
    checked(['uv', 'python', 'install', lock['python']])
    checked(['uv', 'venv', '--python', lock['python'], str(task / 'checker-env')])
    requirements = ROOT / 'scripts/ci/checker-requirements.lock'
    if hashlib.sha256(requirements.read_bytes()).hexdigest() != lock['checker_requirements_sha256']:
        raise ValueError('checker wheel lock changed')
    checked(['uv', 'pip', 'install', '--python', str(task / 'checker-env/bin/python'),
             '--require-hashes', '--only-binary=:all:', '--no-deps', '-r', str(requirements)])
    canonical_files(lock['checker'], task / 'checker')
    sources, binaries = task / 'sources', task / 'bin'
    sources.mkdir()
    binaries.mkdir()
    identities = {}
    for name, revision in lock['repositories'].items():
        checkout = sources / name
        checked(['git', 'init', '--quiet', str(checkout)])
        checked(['git', '-C', str(checkout), 'remote', 'add', 'origin', f'https://github.com/faberline/{name}.git'])
        checked(['git', '-C', str(checkout), 'fetch', '--quiet', '--depth=1', 'origin', revision])
        checked(['git', '-C', str(checkout), 'checkout', '--quiet', '--detach', revision])
        if checked(['git', '-C', str(checkout), 'rev-parse', 'HEAD']) != revision:
            raise ValueError('dependency source identity differs')
        if checked(['git', '-C', str(checkout), 'status', '--porcelain']):
            raise ValueError('dependency source is dirty')
        identities[name] = {'repository': f'faberline/{name}', 'commit': revision}
        if name != 'workspace':
            checked(['cargo', 'build', '--locked', '--manifest-path', str(checkout / 'Cargo.toml'), '--bin', name])
            binary = task / 'target-tools/debug' / name
            data = binary.read_bytes()
            target = binaries / name
            target.write_bytes(data)
            target.chmod(0o700)
            identities[name]['binary_sha256'] = hashlib.sha256(data).hexdigest()
    # These external script paths are used only through the Rust API-double tests.
    if not (sources / 'workspace/acceptance/gcp/scripts/verify-operator-cell.sh').is_file():
        raise ValueError('pinned workspace does not provide the required local oracle')
    (task / 'tool-identities.json').write_text(json.dumps(identities, indent=2) + '\n')
    # Only this fresh hosted task owns this target directory. Keep the copied
    # oracle binaries, release build intermediates before the Sift/Docker gates.
    shutil.rmtree(task / 'target-tools')
    print('Pinned public environment prepared; no cloud commands or registry writes ran.')


if __name__ == '__main__':
    prepare(Path(sys.argv[1]).resolve())
