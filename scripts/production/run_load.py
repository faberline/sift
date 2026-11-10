"""Run one sustained GKE load gate. Other production gates remain required."""
import argparse, concurrent.futures, hashlib, math, os, time, uuid, urllib.parse
from collections import deque
from pathlib import Path
import prepare, verify_runtime
from load_client import Api
from load_data import build, encoded, digest
from gateway_forward import GatewayForward

def require(condition, message):
    if not condition: raise ValueError(message)

def write(path, value):
    raw = encoded(value) + b'\n'
    with Path(path).open('xb') as out:
        os.chmod(path, 0o600); out.write(raw); out.flush(); os.fsync(out.fileno())
    return hashlib.sha256(raw).hexdigest()

def percentile(values):
    require(values, 'no latency samples were measured')
    return sorted(values)[math.ceil(len(values) * .95) - 1]

def live(binding, resource, preflight, kubeconfig):
    spec = resource['spec']
    cluster = verify_runtime.command(['gcloud', 'container', 'clusters', 'describe', spec['gkeClusterName'],
        '--location', spec['gkeLocation'], '--project', spec['gcpProjectId'], '--format=json'])
    kube = ['kubectl', '--kubeconfig=' + str(kubeconfig), '--request-timeout=30s']
    nodes = verify_runtime.command(kube + ['get', 'nodes', '-o', 'json'])
    pods = verify_runtime.command(kube + ['get', 'pods', '--namespace', resource['metadata']['namespace'], '-o', 'json'])
    return verify_runtime.inspect(binding, resource, preflight, cluster, nodes, pods)

def rule_value(project, run, webhook):
    return {'version': 1, 'project': project, 'source': {'kind': 'metrics',
        'expression': 'sum(rate(sift_acceptance_requests_' + run + '[5s]))', 'comparison': 'greater_equal',
        'threshold': 0}, 'hold_seconds': 0, 'no_data': 'unknown', 'webhook': webhook}

def alert_observation(api, rule, previous):
    path = '/api/v1/alerts?project=' + urllib.parse.quote(api.project)
    found, after = [], None
    for _ in range(100):
        page = api.request('GET', path + ('&after=' + after if after else '') + '&limit=1000')
        found.extend(r['value'] for r in page['records'] if r['value'].get('rule_id') == rule)
        after = page.get('next_cursor')
        if after is not None: after = urllib.parse.quote(after)
        if after is None: break
    else: raise ValueError('alert page limit exceeded')
    require(found and all(r['phase'] in ('pending', 'firing') for r in found), 'concurrent alert is missing or unknown')
    latest = max(r['last_evaluated'] for r in found)
    require(latest > previous and time.time() - latest < 45, 'alert scheduler did not advance')
    return latest

def exercise(api, templates, output, seconds, run, environment, rate=10_000, check=None, observe=None):
    # fsync pending inputs before transport. Keep at most four seconds in flight.
    pending = output / 'pending'; pending.mkdir(mode=0o700)
    timings_start, base = time.monotonic(), time.time_ns()
    jobs, count, alerts, previous, last_check = deque(), 0, 0, 0, timings_start
    runtime_job, alert_job = None, None
    ledger = output / 'verified.jsonl'
    def block_job(block):
        futures = [senders.submit(api.send, batch) for batch in block['batches']]
        accepted = sum(future.result() for future in futures)
        require(accepted == block['offered'], 'offered and acknowledged counts differ')
        hashes = api.verify(block, run)
        return {'second': block['second'], 'offered': block['offered'], 'acknowledged': accepted,
                'input_sha256': digest(block), 'read_sha256': hashes}
    def commit_oldest(out):
        nonlocal count
        path, job = jobs.popleft()
        record = job.result(timeout=30)
        out.write(encoded(record) + b'\n'); out.flush(); os.fsync(out.fileno())
        count += record['acknowledged']; path.unlink()
    with ledger.open('xb') as out, concurrent.futures.ThreadPoolExecutor(max_workers=4) as workers, \
            concurrent.futures.ThreadPoolExecutor(max_workers=12) as senders, \
            concurrent.futures.ThreadPoolExecutor(max_workers=2) as supervisors:
        os.chmod(ledger, 0o600)
        for second in range(seconds):
            target = timings_start + second
            while time.monotonic() < target: time.sleep(max(0, min(.2, target - time.monotonic())))
            require(time.monotonic() - target < 1, 'load generator fell behind; catch-up bursts cannot qualify')
            while jobs and jobs[0][1].done(): commit_oldest(out)
            require(len(jobs) < 4, 'read or write backlog exceeded four seconds')
            block = build(run, environment, second, base, templates, rate)
            for records in block['expected'].values():
                for content in records.values():
                    content.update({'project': api.project, 'environment': environment})
            path = pending / f'{second:06d}.json'; write(path, block)
            jobs.append((path, workers.submit(block_job, block)))
            if runtime_job and runtime_job.done(): runtime_job.result(); runtime_job = None
            if alert_job and alert_job.done(): previous = alert_job.result(); alerts += 1; alert_job = None
            if observe and second >= 20 and second % 15 == 5:
                require(alert_job is None, 'alert observer did not finish before the next interval')
                alert_job = supervisors.submit(observe, previous)
            if check and time.monotonic() - last_check >= 60:
                require(runtime_job is None, 'runtime inspection did not finish before the next interval')
                runtime_job = supervisors.submit(check); last_check = time.monotonic()
            if second % 60 == 0: print(f'{second}s offered; {count} items read back', flush=True)
        target = timings_start + seconds
        while time.monotonic() < target: time.sleep(max(0, min(.2, target - time.monotonic())))
        load_elapsed = time.monotonic() - timings_start
        while jobs: commit_oldest(out)
        if runtime_job: runtime_job.result(timeout=60)
        if alert_job: alert_job.result(timeout=30); alerts += 1
    require(count == seconds * rate and seconds <= load_elapsed < seconds + 1, 'sustained load duration or count differs')
    return {'acknowledged_events': count, 'load_elapsed_seconds': load_elapsed, 'alert_observations': alerts,
        'log_query_p95_seconds': percentile(api.timings['logs']),
        'metric_query_p95_seconds': percentile(api.timings['metrics']),
        'trace_read_p95_seconds': percentile(api.timings['traces'])}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['candidate-manifest', 'prepared-dir', 'samples', 'kubeconfig', 'token-file', 'output-dir']:
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--webhook-reference', required=True)
    parser.add_argument('--gate', choices=['load_30m', 'load_24h'], required=True)
    args = parser.parse_args()
    raw, candidate = prepare.read(args.candidate_manifest)
    binding = prepare.candidate_module.validate_candidate('sift', raw, candidate)
    _, resource = prepare.read(args.prepared_dir / 'instance.json')
    _, preflight = prepare.read(args.prepared_dir / 'preflight.json')
    sample_raw, samples = prepare.read(args.samples, 64 * 1024 * 1024)
    require(hashlib.sha256(sample_raw).hexdigest() == preflight['sample_file_sha256'], 'sizing sample bytes differ')
    budget = preflight['budget']
    require(0 < budget['estimated_cost_usd'] <= budget['cap_usd'], 'approved spend cap is missing or exceeded')
    require(binding['version'] == '0.2.0', 'load gate is for Sift 0.2.0')
    runtime = live(binding, resource, preflight, args.kubeconfig)
    require(not args.output_dir.exists(), 'output directory must be new; interrupted runs cannot qualify')
    args.output_dir.mkdir(mode=0o700, parents=True)
    write(args.output_dir / 'runtime-before.json', runtime)
    templates = []
    for entry in samples:
        if entry.get('resource', {}).get('type') != 'k8s_container': continue
        labels = entry['resource'].get('labels', {})
        if not any(labels.get('namespace_name') == source['namespace'] and labels.get('project_id') == source['project']
                   and labels.get('container_name') == source['container'] for source in preflight['sample_sources']): continue
        value = entry.get('textPayload')
        if value is None and 'jsonPayload' in entry: value = encoded(entry['jsonPayload']).decode()
        if value:
            require(len(value.encode()) <= 8192, 'sample exceeds the load driver payload bound')
            templates.append(value)
    require(len(templates) >= 100, 'at least 100 application payloads are required')
    project, environment = resource['metadata']['name'], resource['metadata']['namespace']
    with GatewayForward(args.kubeconfig, environment, runtime['gateway_pod']) as endpoint:
        api = Api(endpoint, project, args.token_file)
        try: run_gate(args, binding, resource, preflight, runtime, templates, api, environment)
        finally: api.close()

def run_gate(args, binding, resource, preflight, runtime, templates, api, environment):
    project, run = api.project, uuid.uuid4().hex
    rule_id, created = 'acceptance-load-' + run, False
    write(args.output_dir / 'run.json', {'gate': args.gate, 'run_id': run, 'source_commit': binding['commit'],
        'image': runtime['image'], 'cluster_identity_sha256': runtime['cluster_identity_sha256'],
        'sample_file_sha256': preflight['sample_file_sha256'], 'started_at': int(time.time()),
        'project': project, 'environment': environment})
    def check():
        report = live(binding, resource, preflight, args.kubeconfig)
        require(report['gateway_pod']==runtime['gateway_pod'], 'bound gateway Pod was replaced')
        write(args.output_dir / ('runtime-' + str(time.time_ns()) + '.json'), report)
    seconds = 1800 if args.gate == 'load_30m' else 86400
    try:
        api.request('PUT', '/api/v1/alert-rules/' + rule_id, {'project': project, 'request_id': run + '-rule',
            'expected_revision': 0, 'value': rule_value(project, run, args.webhook_reference)})
        created = True
        result = exercise(api, templates, args.output_dir, seconds, run, environment,
                          check=check, observe=lambda previous: alert_observation(api, rule_id, previous))
        require(result['alert_observations'] >= seconds // 15 - 3, 'alerts were not observed throughout load')
        require(result['log_query_p95_seconds'] <= 2 and result['metric_query_p95_seconds'] <= 2
                and result['trace_read_p95_seconds'] <= 1, 'query latency exceeded its production limit')
        write(args.output_dir / 'runtime-after.json', live(binding, resource, preflight, args.kubeconfig))
        gate = {key: result[key] for key in ['acknowledged_events', 'log_query_p95_seconds',
            'metric_query_p95_seconds', 'trace_read_p95_seconds']}
        gate.update({'result': 'passed', 'source_commit': binding['commit'], 'image': runtime['image'],
            'cluster_identity_sha256': runtime['cluster_identity_sha256'], 'duration_seconds': seconds,
            'offered_events_per_second': 10_000, 'concurrent_queries': True, 'concurrent_alerts': True,
            'content_differences': 0, 'identity_differences': 0, 'incomplete_queries': 0})
        write(args.output_dir / 'measurement.json', result)
    except BaseException as error:
        write(args.output_dir / 'failed.json', {'result': 'failed', 'error_type': type(error).__name__,
            'production_accepted': False, 'pending_inputs_preserved': True})
        raise
    finally:
        if created:
            # This run owns one immutable rule ID and its single creation revision.
            try:
                api.request('DELETE', '/api/v1/alert-rules/' + rule_id, {'project': project,
                    'request_id': run + '-cleanup', 'expected_revision': 1})
            except BaseException:
                write(args.output_dir / 'cleanup-failed.json', {'result': 'failed', 'rule_id': rule_id})
                raise
    write(args.output_dir / (args.gate + '.json'), gate)
    api.close()
    print('Load gate passed. Fault, delivery, capacity, and source comparison gates remain required.')

if __name__ == '__main__':
    try: main()
    except (ValueError, KeyError, TypeError, OSError) as error: raise SystemExit('load gate failed: ' + str(error))
