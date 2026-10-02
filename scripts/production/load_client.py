"""Bounded public API reads. Partial results and partial exports fail the run."""
import http.client, ipaddress, json, ssl, threading, time, urllib.parse
from pathlib import Path
from load_data import timestamp, compare

class Api:
    def __init__(self, endpoint, project, token_file=None):
        self.url = urllib.parse.urlsplit(endpoint)
        if self.url.username or self.url.password or self.url.query or self.url.fragment or self.url.path not in ('', '/'):
            raise ValueError('endpoint requires only a scheme and host')
        if self.url.scheme not in ('http', 'https') or not self.url.hostname:
            raise ValueError('endpoint requires HTTP or HTTPS')
        if self.url.scheme == 'http':
            try: local = ipaddress.ip_address(self.url.hostname).is_loopback
            except ValueError: local = self.url.hostname == 'localhost'
            if not local: raise ValueError('plain HTTP requires a local port forward')
        self.project, self.token_file = project, Path(token_file) if token_file else None
        self.local, self.timings, self.lock = threading.local(), {'logs': [], 'metrics': [], 'traces': []}, threading.Lock()
        self.connections = []

    def close(self):
        with self.lock:
            for connection in self.connections: connection.close()
            self.connections.clear()

    def request(self, method, path, value=None, latency=None):
        connection = getattr(self.local, 'connection', None)
        if connection is None:
            kind = http.client.HTTPSConnection if self.url.scheme == 'https' else http.client.HTTPConnection
            options = {'context': ssl.create_default_context()} if self.url.scheme == 'https' else {}
            connection = kind(self.url.hostname, self.url.port, timeout=15, **options)
            self.local.connection = connection
            with self.lock: self.connections.append(connection)
        headers = {'Content-Type': 'application/json', 'x-sift-project': self.project}
        if self.token_file:
            if not self.token_file.is_file() or self.token_file.stat().st_size > 16_384:
                raise ValueError('token file must be a bounded regular file')
            token = self.token_file.read_text().strip()
            if not token or any(c.isspace() for c in token): raise ValueError('token file is invalid')
            headers['Authorization'] = 'Bearer ' + token
        body = json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':')).encode() if value is not None else None
        begin = time.monotonic()
        try:
            connection.request(method, path, body, headers)
            response = connection.getresponse()
            raw = response.read(16 * 1024 * 1024 + 1)
            if response.status != 200 or len(raw) > 16 * 1024 * 1024:
                raise ValueError('API refused request with HTTP ' + str(response.status) + ' or exceeded its limit')
            result = json.loads(raw)
            if result.get('partial') or result.get('query_id') or result.get('warnings'):
                raise ValueError('API returned an incomplete or queued query')
        except BaseException:
            connection.close(); self.local.connection = None
            raise
        if latency:
            elapsed = time.monotonic() - begin
            with self.lock: self.timings[latency].append(elapsed)
        return result

    def send(self, batch):
        signal, count, payload = batch
        response = self.request('POST', '/v1/' + signal, payload)
        partial = response.get('partialSuccess', response.get('partial_success', {}))
        if any(int(partial.get(name, 0)) for name in ('rejectedLogRecords', 'rejectedSpans', 'rejectedDataPoints',
                'rejected_log_records', 'rejected_spans', 'rejected_data_points')) or partial.get('errorMessage'):
            raise ValueError('OTLP partial success is not an acknowledgement of the whole batch')
        return count

    def query(self, signal, start, end, name=None, run=None):
        request = {'version': 1, 'project': self.project, 'time_range': {'start': timestamp(start), 'end': timestamp(end)},
                   'signal': {'kind': signal}, 'mode': 'sync', 'limit': 1000}
        if name: request['signal'].update({'name': name, 'function': 'raw'})
        if signal == 'logs':
            request['signal']['filter'] = {'op': 'eq', 'field': 'attributes.run_id.value', 'value': run}
        result, cursors = [], set()
        for _ in range(1000):
            response = self.request('POST', '/api/v1/query', request, signal)
            result.extend(response['data']['records' if signal == 'logs' else 'series'])
            cursor = response.get('next_cursor')
            if cursor is None: return result
            if cursor in cursors: raise ValueError('query cursor did not advance')
            cursors.add(cursor); request['cursor'] = cursor
        raise ValueError('query exceeded its page limit')

    def verify(self, block, run):
        expected = block['expected']; start, end = block['start_nanos'], block['end_nanos']
        results = {}
        logs = self.query('logs', start, end, run=run)
        results['logs'] = compare(expected['logs'], logs, 'logs')
        points = []
        for name in ['sift_acceptance_requests_' + run, 'sift_acceptance_duration_' + run]:
            for series in self.query('metrics', start, end, name):
                if series.get('overflow'): raise ValueError('metric source overflowed')
                for point in series['points']:
                    points.append(dict(point, resource=series['resource'], name=series['name'],
                                       temporality=series['temporality'],
                                       project=series['project'], environment=series['environment']))
        results['metrics'] = compare(expected['metrics'], points, 'metrics')
        traces = []
        ids = sorted({p['trace_id'] for p in expected['traces'].values()})
        for trace_id in ids:
            value = self.request('GET', '/api/v1/traces/' + trace_id + '?project=' + urllib.parse.quote(self.project), latency='traces')
            if value.get('gaps') or value.get('cycles'): raise ValueError('trace is incomplete')
            traces.extend(value['spans'])
        results['traces'] = compare(expected['traces'], traces, 'traces')
        if block['second'] > 0:
            response = self.request('POST', '/api/v2/query', {'version': 2, 'project': self.project,
                'time_range': {'end': timestamp(end)}, 'signal': {'kind': 'metrics',
                'expression': 'sum(rate(sift_acceptance_requests_' + run + '[5s]))'}}, 'metrics')
            if not response['data']['result']: raise ValueError('metric rate query returned no data')
        return results
