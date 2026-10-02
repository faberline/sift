"""Deterministic OTLP inputs. Every offered item has one expected read result."""
import datetime, hashlib, json

def encoded(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':')).encode()

def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()

def timestamp(nanos):
    date = datetime.datetime.fromtimestamp(nanos // 1_000_000_000, datetime.timezone.utc)
    return date.strftime('%Y-%m-%dT%H:%M:%S') + f'.{nanos % 1_000_000_000:09d}Z'

def attributes(values):
    return [{'key': key, 'value': {'stringValue': str(value)}} for key, value in values.items()]

def resource(run, environment, shard):
    return {'service.name': 'sift-acceptance', 'deployment.environment.name': environment,
            'k8s.pod.uid': run + '-' + str(shard), 'run_id': run}

def build(run, environment, second, base_nanos, templates, rate=10_000):
    if type(rate) is not int or rate < 10 or rate > 10_000 or rate % 10:
        raise ValueError('load rate must be a multiple of ten from 10 through 10000')
    at = base_nanos + second * 1_000_000_000
    expected = {'logs': {}, 'metrics': {}, 'traces': {}}
    batches = []
    log_count, metric_count, span_count = rate * 8 // 10, rate // 10, rate // 10
    for first in range(0, log_count, 1000):
        raw, res = [], resource(run, environment, 'logs')
        for index in range(first, min(first + 1000, log_count)):
            identity = f'{run}:log:{second}:{index}'
            body = templates[(second + index) % len(templates)]
            record = {'timeUnixNano': str(at + index), 'observedTimeUnixNano': str(at + index),
                'severityText': 'INFO', 'body': {'stringValue': body},
                'attributes': attributes({'sift.event_id': identity, 'run_id': run})}
            raw.append(record)
            expected['logs'][identity] = {'resource': res, 'body_text': body, 'severity': 'INFO',
                'occurred_at': at + index, 'observed_at': at + index, 'json_payload': record,
                'attributes': {key: {'type':'string','value':value} for key,value in
                    {'sift.event_id':identity,'run_id':run,'otel.log.body':body}.items()}}
        batches.append(('logs', len(raw), {'resourceLogs': [{'resource': {'attributes': attributes(res)},
            'scopeLogs': [{'logRecords': raw}]}]}))
    families = {}
    start = base_nanos + (second // 120) * 120 * 1_000_000_000
    for index in range(metric_count):
        res = resource(run, environment, index % 16)
        sequence = (second % 120) * metric_count + index + 1
        histogram = index % 2 == 1
        name = ('sift_acceptance_duration_' if histogram else 'sift_acceptance_requests_') + run
        point = {'startTimeUnixNano': str(start), 'timeUnixNano': str(at + index)}
        content = {'resource': res, 'name': name, 'temporality':'cumulative','time_unix_nano': at + index,
                   'start_time_unix_nano': start, 'stale': False}
        if histogram:
            point.update({'count': str(sequence * 4), 'sum': sequence * 2.5,
                          'explicitBounds': [0.1, 1.0], 'bucketCounts': [sequence, sequence * 2, sequence]})
            content.update({'value': sequence * 2.5, 'histogram': {'kind': 'explicit', 'count': sequence * 4,
                'sum': sequence * 2.5, 'explicit_bounds': [0.1, 1.0],
                'bucket_counts': [sequence, sequence * 2, sequence], 'zero_count': 0,
                'positive_offset': 0, 'negative_offset': 0}})
        else:
            point['asDouble'] = float(sequence)
            content['value'] = float(sequence)
        identity = f'{res["k8s.pod.uid"]}:{name}:{at + index}'
        expected['metrics'][identity] = content
        family = families.setdefault((index % 16, histogram), (res, name, []))
        family[2].append(point)
    resource_metrics = []
    for (_, histogram), (res, name, points) in families.items():
        data = {'aggregationTemporality': 2, 'dataPoints': points}
        if not histogram: data['isMonotonic'] = True
        resource_metrics.append({'resource': {'attributes': attributes(res)}, 'scopeMetrics': [{'metrics': [
            {'name': name, 'histogram' if histogram else 'sum': data}]}]})
    batches.append(('metrics', metric_count, {'resourceMetrics': resource_metrics}))
    spans, res = [], resource(run, environment, 'traces')
    for index in range(span_count):
        group = index // 100
        trace_id = hashlib.sha256(f'{run}:trace:{second}:{group}'.encode()).hexdigest()[:32]
        span_id = f'{index + 1:016x}'
        parent = f'{group * 100 + 1:016x}' if index % 100 else None
        point = {'traceId': trace_id, 'spanId': span_id, 'name': 'acceptance-span',
            'startTimeUnixNano': str(at), 'endTimeUnixNano': str(at + 1_000_000),
            'attributes': attributes({'run_id': run})}
        if parent: point['parentSpanId'] = parent
        spans.append(point)
        identity = f'{trace_id}:{span_id}'
        expected['traces'][identity] = {'trace_id': trace_id, 'span_id': span_id, 'parent_span_id': parent,
            'name': 'acceptance-span', 'start_time_unix_nano': at, 'end_time_unix_nano': at + 1_000_000,
            'resource': res, 'attributes': {'run_id': {'type': 'string', 'value': run}}, 'events': [], 'links': []}
    batches.append(('traces', span_count, {'resourceSpans': [{'resource': {'attributes': attributes(res)},
        'scopeSpans': [{'spans': spans}]}]}))
    return {'second': second, 'start_nanos': at, 'end_nanos': at + 1_000_000_000,
            'batches': batches, 'expected': expected, 'offered': rate}

def nanos(value):
    # RFC3339 variants can have different fractional precision and UTC spelling.
    value = value.replace('+00:00', 'Z')
    if not value.endswith('Z'): raise ValueError('expected UTC timestamp')
    whole, separator, fraction = value[:-1].partition('.')
    seconds = int(datetime.datetime.fromisoformat(whole).replace(tzinfo=datetime.timezone.utc).timestamp())
    return seconds * 1_000_000_000 + int((fraction if separator else '').ljust(9, '0'))

def compare(expected, actual, signal):
    seen, events = {}, set()
    for record in actual:
        if signal == 'logs': identity = record['event_id']
        elif signal == 'metrics':
            identity = f'{record["resource"]["k8s.pod.uid"]}:{record["name"]}:{record["time_unix_nano"]}'
        else: identity = f'{record["trace_id"]}:{record["span_id"]}'
        if identity in seen or record['event_id'] in events:
            raise ValueError('duplicate ' + signal + ' identity')
        events.add(record['event_id'])
        if identity not in expected: raise ValueError('unexpected ' + signal + ' identity')
        normalized = {}
        for key, value in expected[identity].items():
            observed = record.get(key)
            if key in ('occurred_at', 'observed_at'): observed = nanos(observed)
            if key == 'stale' and observed is None: observed = False
            if observed != value: raise ValueError(signal + ' content differs at ' + key)
            normalized[key] = observed
        seen[identity] = normalized
    if set(seen) != set(expected): raise ValueError('missing ' + signal + ' identities')
    return digest(seen)
