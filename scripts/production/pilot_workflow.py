"""Use public Sift paths to check collection, error states, alert delivery, and restart."""
import json, os, time, uuid
from load_data import attributes, timestamp
from run_load import require


def wait(read, condition, message, seconds=60):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = read()
        if condition(value): return value
        time.sleep(.2)
    raise ValueError(message)


def records(api, collection):
    value = api.request('GET', '/api/v1/' + collection + '?project=' + api.project + '&limit=1000')
    require(value.get('next_cursor') is None, 'pilot control page is incomplete')
    return value['records']


def put(api, collection, identity, value, revision=0):
    return api.request('PUT', '/api/v1/' + collection + '/' + identity, {
        'project': api.project, 'request_id': uuid.uuid4().hex, 'expected_revision': revision, 'value': value})


def metric(api, name, value):
    now = time.time_ns()
    api.send(('metrics', 1, {'resourceMetrics': [{'resource': {'attributes': attributes({
        'service.name': 'sift-pilot', 'deployment.environment.name': 'pilot'})}, 'scopeMetrics': [{'metrics': [{
        'name': name, 'gauge': {'dataPoints': [{'timeUnixNano': str(now), 'asDouble': value}]}}]}]}]}))


def append(path, message, level='INFO', source_id=None):
    value = {'level': level, 'service': 'sift-pilot', 'message': message, 'timestamp': timestamp(time.time_ns()),
        'pilot_source': source_id}
    if level == 'ERROR': value['exception'] = {'type': 'PilotTimeout', 'stacktrace': 'pilot handler 42'}
    raw = (json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n').encode()
    with path.open('ab') as out:
        os.chmod(path, 0o600); out.write(raw); out.flush(); os.fsync(out.fileno())
    return raw.decode()


def collected(api, source_id, expected):
    answer = api.request('POST', '/api/v1/query', {'version': 1, 'project': api.project, 'mode': 'sync',
        'limit': 1000, 'signal': {'kind': 'logs', 'filter': {
            'op': 'eq', 'field': 'attributes.pilot_source.value', 'value': source_id}}})
    require(answer.get('next_cursor') is None, 'pilot collected log page is incomplete')
    rows = answer['data']['records']
    require(len(rows) == len(expected) and len({row['event_id'] for row in rows}) == len(expected),
            'collected records are missing or duplicated')
    actual = sorted(row['json_payload']['raw'] for row in rows)
    require(actual == sorted(expected), 'collected original bytes differ')
    require(all(row['resource']['service.name'] == 'sift-pilot' for row in rows), 'collected resource differs')
    fields = ('project', 'environment', 'occurred_at', 'observed_at', 'severity', 'body_text',
        'json_payload', 'resource', 'attributes')
    return {row['event_id']: {name: row.get(name) for name in fields} for row in rows}


def setup(api, service, receiver, root, run):
    source, state, source_id = root / 'source.jsonl', root / 'collector', 'pilot:' + run
    expected = [append(source, 'pilot request 1 failed', 'ERROR', source_id),
        append(source, 'pilot 正常資料', source_id=source_id)]
    result = service.collect(source, state, source_id)
    require(result.returncode == 0 and json.loads(result.stdout)['accepted'] == 2, 'native collection failed')
    collected(api, source_id, expected)
    group = wait(lambda: records(api, 'error-groups'), lambda rows: len(rows) == 1 and rows[0]['value']['count'] == 1,
        'first error group was not observed')[0]
    api.request('PUT', '/api/v1/error-groups/' + group['id'], {'project': api.project,
        'request_id': uuid.uuid4().hex, 'expected_revision': group['revision'], 'status': 'resolved'})
    expected.append(append(source, 'pilot request 2 failed', 'ERROR', source_id))
    result = service.collect(source, state, source_id)
    require(result.returncode == 0 and json.loads(result.stdout)['accepted'] == 1, 'native source did not resume')
    group = wait(lambda: records(api, 'error-groups'), lambda rows: rows and rows[0]['value']['count'] == 2,
        'error recurrence was not observed')[0]
    require(group['value']['status'] == 'open' and group['value']['recurrence_count'] == 1, 'resolved error did not reopen')
    name, rule_id = 'sift_pilot_health_' + run, 'pilot-' + run
    query = {'version': 2, 'project': api.project, 'signal': {'kind': 'metrics', 'expression': 'sum(' + name + ')'}}
    saved = put(api, 'saved-queries', rule_id, {'request': query})
    metric(api, name, 1)
    rule = put(api, 'alert-rules', rule_id, {'version': 1, 'project': api.project, 'environment': 'pilot',
        'source': {'kind': 'metrics', 'expression': 'sum(' + name + ')', 'comparison': 'greater', 'threshold': .5},
        'hold_seconds': 2, 'no_data': 'unknown', 'webhook': 'pilot'})
    delivered = wait(receiver.snapshot, lambda rows: any(row['status'] == 200 and
        row['payload']['rule_id'] == rule_id and row['payload']['transition'] == 'firing' for row in rows),
        'HTTPS firing delivery did not arrive')
    failed = [row for row in delivered if row['status'] == 503]
    require(len(failed) == 1, 'receiver did not inject one failed attempt')
    require(any(row['status'] == 200 and row['payload']['notification_id'] == failed[0]['payload']['notification_id']
        and row['payload_sha256'] == failed[0]['payload_sha256'] for row in delivered), 'notification retry ID or payload changed')
    metric(api, name, 0)
    wait(receiver.snapshot, lambda rows: any(row['status'] == 200 and row['payload']['rule_id'] == rule_id
        and row['payload']['transition'] == 'resolved' for row in rows), 'HTTPS recovery did not arrive')
    alert = wait(lambda: records(api, 'alerts'), lambda rows: rows and rows[0]['value']['phase'] == 'inactive',
        'alert did not recover')[0]
    until = int(time.time()) + 7200
    api.request('PUT', '/api/v1/alerts/' + alert['id'] + '/mute', {'project': api.project,
        'request_id': uuid.uuid4().hex, 'expected_revision': alert['revision'], 'until': until})
    return {'source': source, 'state': state, 'source_id': source_id, 'expected': expected, 'group_id': group['id'],
        'saved': saved, 'rule': rule, 'alert_id': alert['id'], 'mute_until': until, 'metric_name': name}


def restart(api, service, receiver, context):
    before = collected(api, context['source_id'], context['expected'])
    api.close(); service.stop()
    context['expected'].append(append(context['source'], 'pilot outage buffered record', source_id=context['source_id']))
    rejected = service.collect(context['source'], context['state'], context['source_id'], retries=0)
    require(rejected.returncode != 0, 'offline collector unexpectedly reported success')
    service.start()
    delivered = service.collect(context['source'], context['state'], context['source_id'])
    require(delivered.returncode == 0 and json.loads(delivered.stdout)['accepted'] == 1,
        'collector did not deliver the saved outage record')
    after = collected(api, context['source_id'], context['expected'])
    require(all(after.get(identity) == value for identity, value in before.items()),
        'acknowledged collected content or timestamps changed after restart')
    groups = records(api, 'error-groups')
    require(len(groups) == 1 and groups[0]['id'] == context['group_id'] and groups[0]['value']['count'] == 2,
        'error state changed after restart')
    for collection, key in [('saved-queries', 'saved'), ('alert-rules', 'rule')]:
        require(records(api, collection) == [context[key]], collection + ' changed after restart')
    alerts = records(api, 'alerts')
    require(len(alerts) == 1 and alerts[0]['id'] == context['alert_id']
        and alerts[0]['value']['mute_until'] == context['mute_until'], 'alert mute changed after restart')
    count = len(receiver.snapshot()); metric(api, context['metric_name'], 1)
    wait(lambda: records(api, 'alerts'), lambda rows: rows and rows[0]['value']['phase'] == 'firing',
        'muted alert did not reach its condition')
    require(len(receiver.snapshot()) == count, 'muted alert sent a notification')
    return {'acknowledged_native_records_preserved': len(before), 'outage_records_replayed': 1,
        'error_status_preserved': True, 'saved_query_preserved': True, 'rule_preserved': True, 'mute_verified': True}
