"""Phases of the single-node acceptance after setup. Standard library only."""
import argparse, datetime, http.client, json, math, os, re, subprocess, sys, tempfile, threading, time, urllib.parse, uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'production'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from load_client import Api  # noqa: E402
from load_data import build, timestamp  # noqa: E402
import run_local_load as driver  # noqa: E402
import cost_guard, cost_model  # noqa: E402

PHASES = ('install_one_namespace', 'load_30m', 'load_24h', 'faults', 'comparison', 'retention', 'deleted_pod_logs')
GUARD = Path(__file__).resolve().parents[2] / 'production' / 'cost_guard.py'
GUARD_FILE = 'cost-guard.json'
TTL_MINUTES = 310
SETUP_SECONDS_MAX = 1200
FULL_RUN_HOURS = 5.0
MAX_LOAD_RATE = 1000
LOCAL_PORT = 17380
HEADROOM_SECONDS = 120
COMPARISON_SECONDS = 30
FAULT_RATE = 100
PROJECT = 'single-node-acceptance'
ENDPOINT = f'http://127.0.0.1:{LOCAL_PORT}'
POD = 'sift-all-0'
PVC = 'data-sift-all-0'
SAMPLE_SECONDS = 60
RETENTION_DAYS = 1
RETENTION_ROLL_SECONDS = 900
OLD_EXTRA_DAYS = 1
RETENTION_RATE = 10
AGENT_PROJECT = 'sift'
DELETED_POD_NAME = 'g3-short-lived'
DELETED_POD_MARKER_PREFIX = 'sift-g3-marker-'
DELETED_POD_DELETE_SECONDS = 10
DELETED_POD_WAIT_SECONDS = 120
INSTALL_WAIT_SECONDS = 180
INSTALL_POLL_SECONDS = 10
INSTALL_QUERY = 'sift_collector_pending_records'
GATES = {'retention': {
    'gate': 'G2',
    'proves': 'retentionDays 1 reaches the Pod (--retention-days 1); a batch older than 1 day is rejected at ingest and is not queryable; a batch inside 1 day is accepted and queryable',
    'does_not_prove': 'expiry of stored data (proved by the test of #337), a retention of 30 days, reuse of disk space',
}, 'deleted_pod_logs': {
    'gate': 'G3',
    'proves': 'the log line of a Pod that was deleted within 10 s of its exit is queryable with the Pod name and the namespace',
    'does_not_prove': 'a Pod deleted sooner than 10 s, many short-lived Pods at once, a node restart, or the cost of the checkpoint file',
}, 'install_one_namespace': {
    'gate': 'G1',
    'proves': 'the log records and the metric series of one namespace came through the agents of that namespace; one sift_query call and one PromQL query answered with HTTP 200 and data',
    'does_not_prove': 'an install with more than one node or namespace, GCS, an FQDN policy, query speed, or the volume of the load',
}}


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


def phase_result(phase, start, end, selected, accepted, node_count, detail='', usage=None):
    result = 'pass' if selected > 0 and accepted == selected else 'fail'
    record = {'phase': phase, 'start': start, 'end': end, 'selected': selected,
              'accepted': accepted, 'node_count': node_count, 'result': result, 'detail': detail}
    if usage is not None:
        record.update(usage)
        problems = usage_problems(usage, accepted)
        if problems:
            record['result'] = 'fail'
            joined = '; '.join(problems)
            record['detail'] = f'{detail} {joined}' if detail else joined
    return record


def with_gate(result):
    if result['phase'] not in GATES:
        return result
    return {**result, **GATES[result['phase']]}


def write_evidence(evidence_dir, result):
    result = with_gate(result)
    directory = Path(evidence_dir)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{result['phase']}.json"
    path.write_text(json.dumps(result, indent=2) + '\n')
    return path


def parse_selection(text):
    if text is None or not text.strip():
        return list(PHASES)
    names = [item.strip() for item in text.split(',')]
    seen = set()
    for name in names:
        if not name:
            raise ValueError('empty phase name in the selection')
        if name not in PHASES:
            raise ValueError(f'unknown phase {name!r}')
        if name in seen:
            raise ValueError(f'duplicate phase {name!r}')
        seen.add(name)
    return [phase for phase in PHASES if phase in names]


def acceptance_record(selected):
    return {'mode': 'full' if list(selected) == list(PHASES) else 'partial',
            'executed': list(selected),
            'not_run': [phase for phase in PHASES if phase not in selected]}


def write_acceptance(evidence_dir, selected):
    directory = Path(evidence_dir)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / 'acceptance.json'
    path.write_text(json.dumps(acceptance_record(selected), indent=2) + '\n')
    return path


def projected_hours(cost_inputs, selected):
    # A full run keeps the old fixed estimate. A partial run counts setup and its own phases.
    if list(selected) == list(PHASES):
        return FULL_RUN_HOURS
    hours = cost_inputs['phases_hours']
    return round(hours['setup'] + sum(hours[p] for p in selected), 6)


def guard_figures(cost_inputs, phase, elapsed_seconds, selected=PHASES):
    # Elapsed time counts from the start of run.sh. The setup phase before it is
    # inside the estimate that the aw workflow's guard job already checks.
    config = cost_model.estimate(cost_inputs)['config_values']
    hourly_usd = config['estimated_hourly_cost_usd']
    elapsed_hours = elapsed_seconds / 3600
    spent_usd = config['estimated_fixed_cost_usd'] + hourly_usd * elapsed_hours
    hours_left = sum(cost_inputs['phases_hours'][p] for p in PHASES[PHASES.index(phase):] if p in selected)
    return {'hourly_usd': hourly_usd, 'spent_usd': spent_usd, 'hours_left': hours_left,
            'elapsed_hours': elapsed_hours}


def run_guard(phase, cost_inputs, cap_usd, elapsed_seconds, evidence_dir, selected=PHASES):
    figures = guard_figures(cost_inputs, phase, elapsed_seconds, selected)
    hourly, spent, left = figures['hourly_usd'], figures['spent_usd'], figures['hours_left']
    completed = subprocess.run([sys.executable, '-B', str(GUARD), '--cap-usd', str(cap_usd),
                                '--spent-usd', repr(spent), '--hourly-usd', repr(hourly),
                                '--hours-left', repr(left)], capture_output=True, text=True)
    entry = {'phase': phase, 'elapsed_hours': round(figures['elapsed_hours'], 4),
             'spent_usd': round(spent, 4), 'hourly_usd': hourly, 'hours_left': left,
             'projected_usd': round(cost_guard.projected_cost(spent, hourly, left), 4),
             'cap_usd': str(cap_usd),
             'result': 'pass' if completed.returncode == 0 else 'fail',
             'detail': (completed.stdout + completed.stderr).strip()}
    directory = Path(evidence_dir)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / GUARD_FILE
    entries = json.loads(path.read_text()) if path.exists() else []
    entries.append(entry)
    path.write_text(json.dumps(entries, indent=2) + '\n')
    return completed.returncode


def items_of(block):
    return sum(count for _signal, count, _payload in block['batches'])


def node_count():
    out = subprocess.run(['kubectl', 'get', 'nodes', '-l', 'faberline.dev/pool=sift-server',
                          '--no-headers'], check=True, capture_output=True, text=True).stdout
    return len([line for line in out.splitlines() if line.strip()])


def pod_figures(summary, namespace, pod=POD, pvc=PVC):
    """The CPU, RSS and data volume figures of one Pod in a kubelet stats summary."""
    figures = {'cpu_nanoseconds': None, 'rss_bytes': None, 'volume_used_bytes': None}
    for entry in summary.get('pods') or []:
        ref = entry.get('podRef') or {}
        if ref.get('name') != pod or ref.get('namespace') != namespace:
            continue
        figures['cpu_nanoseconds'] = (entry.get('cpu') or {}).get('usageCoreNanoSeconds')
        figures['rss_bytes'] = (entry.get('memory') or {}).get('rssBytes')
        for volume in entry.get('volume') or []:
            if (volume.get('pvcRef') or {}).get('name') == pvc:
                figures['volume_used_bytes'] = volume.get('usedBytes')
        break
    return figures


def read_pod_figures(namespace, run=subprocess.run):
    node = run(['kubectl', '-n', namespace, 'get', 'pod', POD, '-o', 'jsonpath={.spec.nodeName}'],
               capture_output=True, text=True, check=True).stdout.strip()
    raw = run(['kubectl', 'get', '--raw', f'/api/v1/nodes/{node}/proxy/stats/summary'],
              capture_output=True, text=True, check=True).stdout
    return pod_figures(json.loads(raw), namespace)


def read_or_note(namespace, errors):
    try:
        return read_pod_figures(namespace)
    except (subprocess.CalledProcessError, OSError, ValueError) as error:
        errors.append(str(error))
        return None


class PeakSampler:
    """Reads the Pod figures every `interval` seconds on a daemon thread."""

    def __init__(self, read, interval=SAMPLE_SECONDS):
        self.read = read
        self.interval = interval
        self.samples = []
        self.errors = []
        self.stopped = threading.Event()
        self.thread = None

    def _loop(self):
        while not self.stopped.wait(self.interval):
            try:
                self.samples.append(self.read())
            except Exception as error:  # a failed sample is noted, the phase goes on
                self.errors.append(str(error))

    def start(self):
        self.thread = threading.Thread(target=self._loop, daemon=True)
        self.thread.start()

    def stop(self):
        self.stopped.set()
        if self.thread is not None:
            self.thread.join(timeout=10)
        return self.samples


def _value(reading, key):
    return None if reading is None else reading.get(key)


def usage_figures(first, last, samples, errors=()):
    first_cpu, last_cpu = _value(first, 'cpu_nanoseconds'), _value(last, 'cpu_nanoseconds')
    cpu = None
    if first_cpu is not None and last_cpu is not None and last_cpu >= first_cpu:
        cpu = (last_cpu - first_cpu) / 1e9
    rss = [value for value in [_value(first, 'rss_bytes'), _value(last, 'rss_bytes')]
           + [_value(sample, 'rss_bytes') for sample in samples] if value is not None]
    peak = max(rss) if rss else None
    first_disk, last_disk = _value(first, 'volume_used_bytes'), _value(last, 'volume_used_bytes')
    disk = last_disk - first_disk if first_disk is not None and last_disk is not None else None
    if errors:
        why = errors[0]
    elif first is None or last is None:
        why = 'first or last reading missing'
    else:
        why = 'the kubelet stats have no value (the Pod restarted, or the volume is not listed)'
    parts = [f'{name}: {why}' for name, value in
             (('cpu_seconds', cpu), ('peak_rss_bytes', peak), ('disk_bytes_added', disk))
             if value is None]
    return {'cpu_seconds': cpu, 'peak_rss_bytes': peak, 'disk_bytes_added': disk,
            'not_measured_reason': '; '.join(parts) or None}


def usage_problems(usage, accepted):
    if usage.get('not_measured_reason'):
        return []
    problems = []
    for name in ('cpu_seconds', 'peak_rss_bytes'):
        if usage[name] is None:
            problems.append(f'{name} is null without not_measured_reason')
    if usage['disk_bytes_added'] is None:
        problems.append('disk_bytes_added is null without not_measured_reason')
    elif usage['disk_bytes_added'] == 0 and accepted > 0:
        problems.append(f'disk_bytes_added is 0 for {accepted} accepted items without not_measured_reason')
    return problems


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
    errors = []
    first = read_or_note(namespace, errors)
    sampler = PeakSampler(lambda: read_pod_figures(namespace))
    if first is not None:
        sampler.start()
    try:
        api = Api(ENDPOINT, PROJECT)
        step = driver.run_step(api, uuid.uuid4().hex, PROJECT, rate, load_seconds(hours),
                               driver.synthetic_templates(), time.time_ns(), None,
                               Path(tempfile.mkdtemp()) / 'none')
    finally:
        samples = sampler.stop()
        errors.extend(sampler.errors)
        last = read_or_note(namespace, errors)
        forward.stop()
    usage = usage_figures(first, last, samples, errors)
    result = phase_result(phase, '', '', step['offered_items'], step['accepted_items'], 0,
                          json.dumps(step, sort_keys=True), usage=usage)
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


def retention_days_from_args(args):
    if '--retention-days' not in args:
        return None
    index = args.index('--retention-days') + 1
    if index >= len(args):
        return None
    try:
        return int(args[index])
    except ValueError:
        return None


def check_retention(api, days, now_nanos, templates):
    old_run = uuid.uuid4().hex
    new_run = uuid.uuid4().hex
    old = build(old_run, PROJECT, 0, now_nanos - (days + OLD_EXTRA_DAYS) * 86400 * 1_000_000_000,
                templates, RETENTION_RATE)
    new = build(new_run, PROJECT, 0, now_nanos, templates, RETENTION_RATE)
    selected = items_of(old) + items_of(new)
    accepted = 0
    notes = []
    old_ok = True
    for batch in old['batches']:
        try:
            api.send(batch)
        except ValueError as error:
            if 'partial success' not in str(error):
                notes.append(f'old {batch[0]} batch: {error}')
                old_ok = False
        else:
            notes.append(f'old {batch[0]} batch was accepted')
            old_ok = False
    try:
        rows = api.query('logs', old['start_nanos'], old['end_nanos'], run=old_run)
    except ValueError as error:
        notes.append(f'old query: {error}')
        old_ok = False
    else:
        if rows:
            notes.append('old logs are queryable')
            old_ok = False
    if old_ok:
        accepted += items_of(old)
    try:
        for batch in new['batches']:
            api.send(batch)
        api.verify(new, new_run)
        accepted += items_of(new)
    except ValueError as error:
        notes.append(f'new: {error}')
    return selected, accepted, notes


def retention_result(selected, accepted, notes):
    return phase_result('retention', '', '', selected, accepted, 0,
                        '; '.join(notes) or 'old batch rejected and not queryable; new batch accepted and queryable')


def pod_manifest(namespace, marker):
    return {
        'apiVersion': 'v1',
        'kind': 'Pod',
        'metadata': {'name': DELETED_POD_NAME, 'namespace': namespace},
        'spec': {
            'restartPolicy': 'Never',
            'nodeSelector': {'faberline.dev/pool': 'sift-server'},
            'tolerations': [{'key': 'faberline.dev/pool', 'operator': 'Equal',
                             'value': 'sift-server', 'effect': 'NoSchedule'}],
            'containers': [{'name': 'writer', 'image': 'busybox:1.36.1',
                            'command': ['sh', '-c', f'echo {marker}']}],
        },
    }


def find_marker(api, marker, namespace, pod, now_nanos):
    """The records that carry the marker with the Pod name and the namespace of the Pod."""
    hour = 3600 * 1_000_000_000
    request = {'version': 1, 'project': AGENT_PROJECT,
               'time_range': {'start': timestamp(now_nanos - hour), 'end': timestamp(now_nanos + hour // 6)},
               'signal': {'kind': 'logs',
                          'filter': {'op': 'eq', 'field': 'resource.k8s.pod.name', 'value': pod}},
               'mode': 'sync', 'limit': 1000}
    records, cursors = [], set()
    for _ in range(1000):
        response = api.request('POST', '/api/v1/query', request, 'logs')
        records.extend(response['data']['records'])
        cursor = response.get('next_cursor')
        if cursor is None:
            return [record for record in records
                    if marker in record['body_text']
                    and record['resource'].get('k8s.pod.name') == pod
                    and record['resource'].get('k8s.namespace.name') == namespace]
        if cursor in cursors:
            raise ValueError('log query cursor did not advance')
        cursors.add(cursor)
        request['cursor'] = cursor
    raise ValueError('log query exceeded its page limit')


def check_deleted_pod_logs(api, marker, namespace, pod, now_nanos,
                           wait_seconds=DELETED_POD_WAIT_SECONDS, sleep=time.sleep):
    waited = 0
    error = None
    while True:
        try:
            matches = find_marker(api, marker, namespace, pod, now_nanos)
            error = None
        except ValueError as failure:
            matches, error = [], str(failure)
        if matches or waited >= wait_seconds:
            break
        sleep(5)
        waited += 5
    notes = []
    if not matches:
        notes.append(f'marker not found within {waited} s')
        if error:
            notes.append(f'last query error: {error}')
    elif len(matches) > 1:
        notes.append(f'marker found {len(matches)} times')
    return 1, (1 if len(matches) == 1 else 0), notes


def deleted_pod_logs_result(selected, accepted, notes):
    return phase_result('deleted_pod_logs', '', '', selected, accepted, 0,
                        '; '.join(notes) or 'the marker of a deleted Pod was found with its Pod name and namespace')


def wait_pod_succeeded(namespace, seconds=DELETED_POD_WAIT_SECONDS):
    deadline = time.monotonic() + seconds
    while True:
        phase = subprocess.run(['kubectl', '-n', namespace, 'get', 'pod', DELETED_POD_NAME, '-o',
                                'jsonpath={.status.phase}'], capture_output=True, text=True).stdout.strip()
        if phase == 'Succeeded':
            return time.monotonic()
        if time.monotonic() >= deadline:
            raise RuntimeError(f'{DELETED_POD_NAME} did not reach Succeeded in {seconds} s')
        time.sleep(2)


def run_deleted_pod_logs(namespace):
    marker = DELETED_POD_MARKER_PREFIX + uuid.uuid4().hex
    subprocess.run(['kubectl', '-n', namespace, 'apply', '-f', '-'],
                   input=json.dumps(pod_manifest(namespace, marker)), text=True, check=True)
    selected, accepted, notes = 1, 0, []
    forward = Forward()
    try:
        exited = wait_pod_succeeded(namespace)
        delay = time.monotonic() - exited
        subprocess.run(['kubectl', '-n', namespace, 'delete', 'pod', DELETED_POD_NAME, '--wait=false'],
                       check=True)
        forward.start(namespace)
        selected, accepted, notes = check_deleted_pod_logs(
            Api(ENDPOINT, AGENT_PROJECT), marker, namespace, DELETED_POD_NAME, time.time_ns())
        if delay > DELETED_POD_DELETE_SECONDS:
            accepted = 0
            notes.insert(0, f'delete started later than {DELETED_POD_DELETE_SECONDS} s after exit')
    finally:
        forward.stop()
        subprocess.run(['kubectl', '-n', namespace, 'delete', 'pod', DELETED_POD_NAME,
                        '--ignore-not-found', '--wait=false'])
    return deleted_pod_logs_result(selected, accepted, notes)


def wait_retention_args(namespace, days, seconds=RETENTION_ROLL_SECONDS, run=subprocess.run):
    deadline = time.monotonic() + seconds
    while True:
        read = run(['kubectl', '-n', namespace, 'get', 'pod', POD, '-o',
                    'jsonpath={.spec.containers[0].args}'], capture_output=True, text=True)
        args = None
        if read.returncode == 0:
            try:
                args = json.loads(read.stdout)
            except (ValueError, TypeError):
                args = None
        if args is not None and retention_days_from_args(args) == days:
            return
        if time.monotonic() >= deadline:
            raise RuntimeError(f'the Pod args do not show --retention-days {days} in {seconds} s')
        time.sleep(5)


def run_retention(namespace):
    subprocess.run(['kubectl', '-n', namespace, 'patch', 'sift', 'sift', '--type', 'merge', '-p',
                    json.dumps({'spec': {'retentionDays': RETENTION_DAYS}})], check=True)
    deadline = time.monotonic() + RETENTION_ROLL_SECONDS
    wait_retention_args(namespace, RETENTION_DAYS, seconds=max(1, deadline - time.monotonic()))
    wait_pod_ready(namespace, max(1, deadline - time.monotonic()))
    forward = Forward()
    forward.start(namespace)
    try:
        api = Api(ENDPOINT, PROJECT)
        selected, accepted, notes = check_retention(api, RETENTION_DAYS, time.time_ns(),
                                                    driver.synthetic_templates())
    finally:
        forward.stop()
    return retention_result(selected, accepted, notes)


def http_status(error):
    match = re.search(r'HTTP (\d+)', str(error))
    return int(match.group(1)) if match else 0


def count_namespace_logs(api, namespace, now_nanos):
    """The log records of one namespace in the last hour and ten minutes ahead."""
    hour = 3600 * 1_000_000_000
    request = {'version': 1, 'project': AGENT_PROJECT,
               'time_range': {'start': timestamp(now_nanos - hour), 'end': timestamp(now_nanos + hour // 6)},
               'signal': {'kind': 'logs',
                          'filter': {'op': 'eq', 'field': 'resource.k8s.namespace.name', 'value': namespace}},
               'mode': 'sync', 'limit': 1000}
    records, cursors = [], set()
    for _ in range(1000):
        response = api.request('POST', '/api/v1/query', request, 'logs')
        records.extend(response['data']['records'])
        cursor = response.get('next_cursor')
        if cursor is None:
            return len([record for record in records
                        if record['resource'].get('k8s.namespace.name') == namespace])
        if cursor in cursors:
            raise ValueError('log query cursor did not advance')
        cursors.add(cursor)
        request['cursor'] = cursor
    raise ValueError('log query exceeded its page limit')


def count_metric_series(api, namespace):
    query = urllib.parse.urlencode({'project': AGENT_PROJECT, 'environment': namespace,
                                    'query': INSTALL_QUERY})
    response = api.request('GET', '/prometheus/api/v1/query?' + query, None, 'metrics')
    return len(response['data']['result'])


def check_install_one_namespace(api, namespace, now_nanos,
                                wait_seconds=INSTALL_WAIT_SECONDS, sleep=time.sleep):
    waited = 0
    while True:
        sift_status = promql_status = 200
        sift_error = promql_error = None
        try:
            log_records = count_namespace_logs(api, namespace, now_nanos)
        except ValueError as error:
            log_records, sift_status, sift_error = 0, http_status(error), str(error)
        try:
            metric_series = count_metric_series(api, namespace)
        except ValueError as error:
            metric_series, promql_status, promql_error = 0, http_status(error), str(error)
        if (log_records > 0 and metric_series > 0) or waited >= wait_seconds:
            break
        sleep(INSTALL_POLL_SECONDS)
        waited += INSTALL_POLL_SECONDS
    notes = []
    if log_records == 0:
        notes.append(f'no log record for the namespace within {waited} s')
    if metric_series == 0:
        notes.append(f'no metric series for the namespace within {waited} s')
    if sift_error:
        notes.append(f'sift_query failed: {sift_error}')
    if promql_error:
        notes.append(f'PromQL failed: {promql_error}')
    return log_records, metric_series, sift_status, promql_status, notes


def install_one_namespace_result(namespace, log_records, metric_series, sift_query_status,
                                 promql_status, notes):
    checks = [log_records > 0, metric_series > 0, sift_query_status == 200, promql_status == 200]
    detail = '; '.join(notes) or 'logs and metrics of the namespace came through its agents; sift_query and PromQL answered'
    result = phase_result('install_one_namespace', '', '', len(checks), sum(checks), 0, detail)
    result.update(namespace=namespace, log_records=log_records, metric_series=metric_series,
                  sift_query_status=sift_query_status, promql_status=promql_status)
    return result


def run_install_one_namespace(namespace):
    forward = Forward()
    forward.start(namespace)
    try:
        api = Api(ENDPOINT, AGENT_PROJECT)
        checked = check_install_one_namespace(api, namespace, time.time_ns())
    finally:
        forward.stop()
    return install_one_namespace_result(namespace, *checked)


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
    guard = sub.add_parser('guard')
    guard.add_argument('phase')
    guard.add_argument('--cost-inputs', required=True, type=Path)
    guard.add_argument('--cap-usd', required=True)
    guard.add_argument('--elapsed-seconds', required=True, type=float)
    guard.add_argument('--evidence-dir', required=True, type=Path)
    guard.add_argument('--phases', default='')
    hours = sub.add_parser('hours')
    hours.add_argument('--phases', default='')
    hours.add_argument('--cost-inputs', required=True, type=Path)
    select = sub.add_parser('select')
    select.add_argument('--phases', default='')
    acceptance = sub.add_parser('acceptance')
    acceptance.add_argument('--phases', default='')
    acceptance.add_argument('--evidence-dir', required=True, type=Path)
    args = parser.parse_args(argv)

    if args.command == 'list':
        print('\n'.join(PHASES))
        return 0
    if args.command == 'hours':
        try:
            selected = parse_selection(args.phases)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        print(projected_hours(json.loads(args.cost_inputs.read_text()), selected))
        return 0
    if args.command in ('select', 'acceptance'):
        try:
            selected = parse_selection(args.phases)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        if args.command == 'select':
            print('\n'.join(selected))
            return 0
        print(write_acceptance(args.evidence_dir, selected))
        return 0
    if args.command == 'timeout':
        print(phase_timeout_seconds(read_hours(args.cost_inputs)[args.phase]))
        return 0
    if args.command == 'fail':
        now = utc_now()
        print(write_evidence(args.evidence_dir, phase_result(args.phase, now, now, 0, 0, 0, args.reason)))
        return 0
    if args.command == 'guard':
        if args.phase not in PHASES:
            parser.error(f'unknown phase {args.phase!r}')
        try:
            selected = parse_selection(args.phases)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        cost = json.loads(args.cost_inputs.read_text())
        code = run_guard(args.phase, cost, args.cap_usd, args.elapsed_seconds, args.evidence_dir, selected)
        print(Path(args.evidence_dir) / GUARD_FILE)
        return code

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
        elif args.phase == 'retention':
            result = run_retention(namespace)
        elif args.phase == 'install_one_namespace':
            result = run_install_one_namespace(namespace)
        elif args.phase == 'deleted_pod_logs':
            result = run_deleted_pod_logs(namespace)
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
