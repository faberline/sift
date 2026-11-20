"""Phases of the single-node acceptance after setup. Standard library only."""
import argparse, datetime, http.client, json, math, os, subprocess, sys, tempfile, time, uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'production'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from load_client import Api  # noqa: E402
from load_data import build  # noqa: E402
import run_local_load as driver  # noqa: E402

PHASES = ('load_30m', 'load_24h', 'faults', 'comparison')
TTL_MINUTES = 300
SETUP_SECONDS_MAX = 1200
MAX_LOAD_RATE = 1000
LOCAL_PORT = 17380
HEADROOM_SECONDS = 120
COMPARISON_SECONDS = 30
FAULT_RATE = 100
PROJECT = 'single-node-acceptance'
ENDPOINT = f'http://127.0.0.1:{LOCAL_PORT}'


def read_hours(path):
    return json.loads(Path(path).read_text())['phases_hours']


def phase_timeout_seconds(hours):
    return math.ceil(round(hours * 3600 * 1.2, 6))


def load_seconds(hours):
    return int(hours * 3600) - HEADROOM_SECONDS


def load_rate(cost_inputs):
    rate = cost_inputs['rated_items_per_second']
    if type(rate) is not int or rate < 10 or rate > MAX_LOAD_RATE or rate % 10:
        raise ValueError(f'rate {rate!r} must be a multiple of 10 from 10 through {MAX_LOAD_RATE}')
    return rate


def check_total(hours, ttl_minutes):
    total = SETUP_SECONDS_MAX + sum(phase_timeout_seconds(hours[p]) for p in PHASES)
    if total >= ttl_minutes * 60:
        raise ValueError('the phases do not fit the ttl')


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def phase_result(phase, start, end, selected, accepted, node_count, detail=''):
    result = 'pass' if selected > 0 and accepted == selected else 'fail'
    return {'phase': phase, 'start': start, 'end': end, 'selected': selected,
            'accepted': accepted, 'node_count': node_count, 'result': result, 'detail': detail}


def write_evidence(evidence_dir, result):
    directory = Path(evidence_dir)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{result['phase']}.json"
    path.write_text(json.dumps(result, indent=2) + '\n')
    return path


def items_of(block):
    return sum(count for _signal, count, _payload in block['batches'])


def node_count():
    out = subprocess.run(['kubectl', 'get', 'nodes', '-l', 'faberline.dev/pool=sift-server',
                          '--no-headers'], check=True, capture_output=True, text=True).stdout
    return len([line for line in out.splitlines() if line.strip()])


class Forward:
    """A kubectl port-forward to the sift Service on the loopback address."""

    def __init__(self):
        self.process = None

    def start(self, namespace):
        if self.process is not None:
            return
        port = subprocess.run(['kubectl', '-n', namespace, 'get', 'svc', 'sift', '-o',
                               'jsonpath={.spec.ports[?(@.name=="http")].port}'],
                              check=True, capture_output=True, text=True).stdout.strip()
        self.process = subprocess.Popen(['kubectl', '-n', namespace, 'port-forward', 'svc/sift',
                                         f'{LOCAL_PORT}:{port}'],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            try:
                connection = http.client.HTTPConnection('127.0.0.1', LOCAL_PORT, timeout=2)
                connection.request('GET', '/readyz')
                ready = connection.getresponse().status == 200
                connection.close()
                if ready:
                    return
            except OSError:
                pass
            time.sleep(1)
        self.stop()
        raise RuntimeError('the port-forward to the sift Service is not ready')

    def stop(self):
        if self.process is None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.process = None


def wait_pod_ready(namespace, seconds=900):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        status = subprocess.run(['kubectl', '-n', namespace, 'get', 'pod', 'sift-all-0', '-o',
                                 'jsonpath={.status.conditions[?(@.type=="Ready")].status}'],
                                capture_output=True, text=True).stdout.strip()
        if status == 'True':
            return
        time.sleep(5)
    raise RuntimeError('sift-all-0 is not ready')


def run_load(phase, hours, rate, namespace):
    forward = Forward()
    forward.start(namespace)
    try:
        api = Api(ENDPOINT, PROJECT)
        step = driver.run_step(api, uuid.uuid4().hex, PROJECT, rate, load_seconds(hours),
                               driver.synthetic_templates(), time.time_ns(), None,
                               Path(tempfile.mkdtemp()) / 'none')
    finally:
        forward.stop()
    result = phase_result(phase, '', '', step['offered_items'], step['accepted_items'], 0,
                          json.dumps(step, sort_keys=True))
    if step['query_errors'] > 0:
        result['result'] = 'fail'
        result['detail'] += f" query_errors={step['query_errors']}"
    return result


def run_faults(namespace):
    forward = Forward()
    forward.start(namespace)
    selected = accepted = 0
    notes = []
    try:
        for name, delete in (('graceful', ['delete', 'pod', 'sift-all-0']),
                             ('forced', ['delete', 'pod', 'sift-all-0',
                                         '--grace-period=0', '--force'])):
            run = uuid.uuid4().hex
            block = build(run, PROJECT, 0, time.time_ns(), driver.synthetic_templates(), FAULT_RATE)
            selected += items_of(block)
            api = Api(ENDPOINT, PROJECT)
            try:
                for batch in block['batches']:
                    api.send(batch)
                api.verify(block, run)
            except ValueError as error:
                notes.append(f'{name} before restart: {error}')
                continue
            subprocess.run(['kubectl', '-n', namespace] + delete, check=True)
            forward.stop()
            wait_pod_ready(namespace)
            forward.start(namespace)
            try:
                Api(ENDPOINT, PROJECT).verify(block, run)
                accepted += items_of(block)
            except ValueError as error:
                notes.append(f'{name} after restart: {error}')
    finally:
        forward.stop()
    return phase_result('faults', '', '', selected, accepted, 0,
                        '; '.join(notes) or 'graceful and forced restarts verified')


def run_comparison(namespace):
    forward = Forward()
    forward.start(namespace)
    notes = []
    try:
        api = Api(ENDPOINT, PROJECT)
        run = uuid.uuid4().hex
        base = time.time_ns()
        blocks = []
        for second in range(COMPARISON_SECONDS):
            block = build(run, PROJECT, second, base, driver.synthetic_templates(), FAULT_RATE)
            blocks.append(block)
            try:
                for batch in block['batches']:
                    api.send(batch)
            except ValueError as error:
                notes.append(f'second {second} send: {error}')
        selected = sum(items_of(block) for block in blocks)
        accepted = 0
        for second, block in enumerate(blocks):
            try:
                api.verify(block, run)
                accepted += items_of(block)
            except ValueError as error:
                notes.append(f'second {second}: {error}')
    finally:
        forward.stop()
    return phase_result('comparison', '', '', selected, accepted, 0,
                        '; '.join(notes) or 'every offered item read back')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('list')
    timeout = sub.add_parser('timeout')
    timeout.add_argument('phase')
    timeout.add_argument('--cost-inputs', required=True, type=Path)
    run = sub.add_parser('run')
    run.add_argument('phase')
    run.add_argument('--cost-inputs', required=True, type=Path)
    run.add_argument('--evidence-dir', required=True, type=Path)
    fail = sub.add_parser('fail')
    fail.add_argument('phase')
    fail.add_argument('--evidence-dir', required=True, type=Path)
    fail.add_argument('--reason', required=True)
    args = parser.parse_args(argv)

    if args.command == 'list':
        print('\n'.join(PHASES))
        return 0
    if args.command == 'timeout':
        print(phase_timeout_seconds(read_hours(args.cost_inputs)[args.phase]))
        return 0
    if args.command == 'fail':
        now = utc_now()
        print(write_evidence(args.evidence_dir, phase_result(args.phase, now, now, 0, 0, 0, args.reason)))
        return 0

    if args.phase not in PHASES:
        parser.error(f'unknown phase {args.phase!r}')
    hours = read_hours(args.cost_inputs)
    try:
        check_total(hours, TTL_MINUTES)
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1
    namespace = os.environ.get('NAMESPACE')
    if not namespace:
        print('NAMESPACE is not set', file=sys.stderr)
        return 1

    start = utc_now()
    try:
        if args.phase in ('load_30m', 'load_24h'):
            cost = json.loads(args.cost_inputs.read_text())
            result = run_load(args.phase, hours[args.phase], load_rate(cost), namespace)
        elif args.phase == 'faults':
            result = run_faults(namespace)
        else:
            result = run_comparison(namespace)
    except Exception as error:  # a phase that raises is a failed phase, with the reason
        result = phase_result(args.phase, start, utc_now(), 0, 0, 0, str(error))
    result.update(start=start, end=utc_now(), node_count=node_count())
    path = write_evidence(args.evidence_dir, result)
    print(path)
    return 0 if result['result'] == 'pass' else 1


if __name__ == '__main__':
    raise SystemExit(main())
