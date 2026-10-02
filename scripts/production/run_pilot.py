#!/usr/bin/env python3
"""Run a small local trial. A passed trial is not rated GKE production acceptance."""
import argparse, hashlib, json, os, re, signal, subprocess, time, uuid
from pathlib import Path
from load_client import Api
from load_data import build, digest, encoded
from pilot_runtime import LocalSift, Receiver
from pilot_workflow import setup, restart, metric, records
from run_load import require, write, percentile


def limits(seconds, rate):
    require(type(seconds) is int and 1 <= seconds <= 1800, 'pilot duration must be from 1 through 1800 seconds')
    require(type(rate) is int and 10 <= rate <= 100 and rate % 10 == 0,
        'pilot rate must be a multiple of ten from 10 through 100')


def input_block(run, second, base, rate, project):
    block = build(run, 'pilot', second, base, ['pilot sample 雨', '{"request":"pilot","status":"ok"}'], rate)
    for rows in block['expected'].values():
        for row in rows.values(): row.update({'project': project, 'environment': 'pilot'})
    # Use the exact JSON shape that the durable input file will contain.
    return json.loads(encoded(block))


def stream(api, service, receiver, context, root, run, seconds, rate):
    inputs = root / 'inputs'; inputs.mkdir(mode=0o700)
    base, begin, restarted = time.time_ns(), time.monotonic(), False
    count, largest_delay, recovery, gauge = 0, 0.0, None, 0
    with (root / 'verified.jsonl').open('xb') as ledger:
        os.chmod(ledger.name, 0o600)
        for second in range(seconds):
            target = begin + second
            if time.monotonic() < target: time.sleep(target - time.monotonic())
            largest_delay = max(largest_delay, time.monotonic() - target)
            require(largest_delay < 5, 'pilot producer fell more than five seconds behind')
            block = input_block(run, second, base, rate, api.project)
            write(inputs / f'{second:06d}.json', block)
            accepted = sum(api.send(batch) for batch in block['batches'])
            require(accepted == rate, 'pilot offered and acknowledged counts differ')
            read = api.verify(block, run)
            result = {'second': second, 'offered': rate, 'acknowledged': accepted,
                'input_sha256': digest(block), 'read_sha256': read}
            ledger.write(encoded(result) + b'\n'); ledger.flush(); os.fsync(ledger.fileno())
            count += accepted
            if second % 30 == 0:
                metric(api, context['metric_name'], 1); gauge += 1
                alerts = records(api, 'alerts')
                require(len(alerts) == 1 and alerts[0]['value']['phase'] in ('pending', 'firing'),
                    'pilot alert evaluation became missing, incomplete, or unknown')
            if seconds >= 2 and second + 1 == seconds // 2:
                pause = time.monotonic()
                recovery = restart(api, service, receiver, context)
                # Restart is a planned pause. It does not become a catch-up burst.
                begin += time.monotonic() - pause; restarted = True
                for old in range(second + 1):
                    previous = json.loads((inputs / f'{old:06d}.json').read_bytes())
                    require(previous == input_block(run, old, base, rate, api.project), 'saved producer input changed')
                    api.verify(previous, run)
                begin = time.monotonic() - (second + 1)
            if second % 60 == 0:
                print(json.dumps({'phase': 'pilot_load', 'seconds_written': second + 1,
                    'acknowledged_items': count, 'restart_verified': restarted}), flush=True)
        target = begin + seconds
        if time.monotonic() < target: time.sleep(target - time.monotonic())
    # Verify every item again after the full run, including all pre-restart inputs.
    verified = 0
    for second in range(seconds):
        block = json.loads((inputs / f'{second:06d}.json').read_bytes())
        require(block == input_block(run, second, base, rate, api.project), 'final producer input changed')
        api.verify(block, run); verified += block['offered']
    require(count == verified == seconds * rate, 'pilot complete read-back count differs')
    return {'offered_items': count, 'acknowledged_items': count, 'verified_items': verified,
        'seconds_written': seconds, 'offered_items_per_second': rate, 'largest_producer_delay_seconds': largest_delay,
        'control_metric_refreshes': gauge, 'restart': recovery,
        'query_p95_seconds': {name: percentile(values) for name, values in api.timings.items()}}


def run(binary, output, seconds=1800, rate=100, receiver_image=None):
    limits(seconds, rate)
    binary = Path(binary).resolve()
    require(binary.is_file() and not binary.is_symlink(), 'pilot binary must be a regular file')
    build_info = json.loads(subprocess.check_output([str(binary), 'acceptance-build-info'], timeout=10))
    require(build_info.get('version') == '0.2.0' and re.fullmatch('[0-9a-f]{40}', build_info.get('git_sha', '')),
        'pilot binary source identity is missing')
    require(not output.exists(), 'pilot output directory must be new')
    output.mkdir(mode=0o700, parents=True)
    run_id, project = uuid.uuid4().hex, 'sift-pilot-' + uuid.uuid4().hex[:12]
    write(output / 'plan.json', {'schema': 'sift.pilot-plan/v1', 'source_commit': build_info['git_sha'],
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'run_id': run_id,
        'environment': 'local', 'seconds': seconds, 'rate': rate, 'production_accepted': False})
    receiver, service, api = None, None, None
    began = time.time()
    try:
        receiver = Receiver(output / 'receiver', docker_image=receiver_image)
        service = LocalSift(binary, output / 'server', project, receiver); service.start()
        api = Api(service.endpoint, project, service.token_file)
        context = setup(api, service, receiver, output, run_id)
        # Check mute and restart before sustained writes too.
        initial_restart = restart(api, service, receiver, context)
        result = stream(api, service, receiver, context, output, run_id, seconds, rate)
        result.update({'schema': 'sift.pilot-result/v1', 'result': 'passed', 'environment': 'local',
            'source_commit': build_info['git_sha'], 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'production_accepted': False, 'rated_items_per_second': None, 'initial_restart': initial_restart,
            'native_logs_verified': len(context['expected']), 'error_recurrence_verified': True,
            'https_firing_and_recovery_verified': True, 'notification_retry_identity_verified': True,
            'wall_seconds': time.time() - began})
        api.close(); service.stop()
        files = [path for path in (output / 'server' / 'data').rglob('*') if path.is_file() and not path.is_symlink()]
        result['measured_local_data_bytes'] = sum(path.stat().st_size for path in files)
        result['stored_data_files'] = len(files)
        write(output / 'result.json', result)
        print(json.dumps(result, sort_keys=True), flush=True)
        return result
    except BaseException as error:
        if api:
            diagnostic = {}
            for collection in ('error-groups', 'alert-rules', 'alerts', 'saved-queries'):
                try: diagnostic[collection] = records(api, collection)
                except (ValueError, OSError): diagnostic[collection] = {'read': 'failed'}
            write(output / 'diagnostic-control.json', diagnostic)
        write(output / 'failure.json', {'schema': 'sift.pilot-result/v1', 'result': 'failed',
            'production_accepted': False, 'source_commit': build_info['git_sha'], 'error_type': type(error).__name__})
        raise
    finally:
        if api: api.close()
        if service: service.stop()
        if receiver: receiver.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=1800); parser.add_argument('--rate', type=int, default=100)
    parser.add_argument('--receiver-image', help='immutable local Python image for Docker HTTPS on macOS')
    args = parser.parse_args()
    def interrupted(*_): raise KeyboardInterrupt('pilot interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    run(args.binary, args.output_dir, args.seconds, args.rate, args.receiver_image)


if __name__ == '__main__':
    try: main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit('pilot refused: ' + str(error))
