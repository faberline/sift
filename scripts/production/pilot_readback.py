"""Compare every returned identity and field with sealed inputs using bounded RAM."""
import json, os, sqlite3, urllib.parse
from load_data import compare, encoded, timestamp
from run_load import require


def identity(record, signal):
    if signal == 'logs': return record['event_id']
    if signal == 'metrics':
        return f'{record["resource"]["k8s.pod.uid"]}:{record["name"]}:{record["time_unix_nano"]}'
    return f'{record["trace_id"]}:{record["span_id"]}'


def accept(db, signal, record):
    require(isinstance(record.get('event_id'), str) and bool(record['event_id']),
        'final record has no physical event identity')
    key = identity(record, signal)
    found = db.execute('SELECT value, seen FROM expected WHERE signal=? AND identity=?', (signal, key)).fetchone()
    require(found is not None, 'unexpected ' + signal + ' identity in final readback')
    require(found[1] == 0, 'duplicate ' + signal + ' identity in final readback')
    compare({key: json.loads(found[0])}, [record], signal)
    try:
        db.execute('UPDATE expected SET seen=1, event_id=? WHERE signal=? AND identity=?',
            (record['event_id'], signal, key))
    except sqlite3.IntegrityError as error:
        raise ValueError('duplicate physical ' + signal + ' event in final readback') from error


def readback(api, blocks, run, path):
    descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_RDWR, 0o600); os.close(descriptor)
    db = sqlite3.connect(path)
    try:
        db.executescript('''PRAGMA cache_size=-8192;
            CREATE TABLE expected (signal TEXT, identity TEXT, value BLOB, trace_id TEXT,
                seen INTEGER NOT NULL DEFAULT 0, event_id TEXT,
                PRIMARY KEY (signal,identity), UNIQUE (signal,event_id));
            CREATE INDEX trace_ids ON expected(trace_id);''')
        start, end, count, samples = None, None, 0, []
        for block in blocks:
            require(sum(len(rows) for rows in block['expected'].values()) == block['offered'],
                'sealed expected and offered counts differ')
            start = block['start_nanos'] if start is None else min(start, block['start_nanos'])
            end = block['end_nanos'] if end is None else max(end, block['end_nanos'])
            samples.append((block['second'], block['end_nanos']))
            for signal, rows in block['expected'].items():
                for key, value in rows.items():
                    db.execute('INSERT INTO expected(signal,identity,value,trace_id) VALUES(?,?,?,?)',
                        (signal, key, encoded(value), value.get('trace_id')))
                    count += 1
        require(count > 0, 'final readback has no sealed input')
        db.commit()
        with db:
            for page in api.log_pages(start, end, run):
                for record in page: accept(db, 'logs', record)
            for name in ('sift_acceptance_requests_' + run, 'sift_acceptance_duration_' + run):
                for series in api.query('metrics', start, end, name):
                    require(not series.get('overflow'), 'final metric source overflowed')
                    for point in series['points']:
                        accept(db, 'metrics', dict(point, resource=series['resource'], name=series['name'],
                            temporality=series['temporality'], project=series['project'], environment=series['environment']))
            for (trace_id,) in db.execute('SELECT DISTINCT trace_id FROM expected WHERE trace_id IS NOT NULL ORDER BY trace_id'):
                value = api.request('GET', '/api/v1/traces/' + trace_id + '?project=' + urllib.parse.quote(api.project), latency='traces')
                require(not value.get('gaps') and not value.get('cycles'), 'final trace is incomplete')
                for span in value['spans']: accept(db, 'traces', span)
        require(db.execute('SELECT COUNT(*) FROM expected WHERE seen=0').fetchone()[0] == 0,
            'final readback is missing expected records')
        # Streaming already checks rate on every second. Check old and recent
        # evaluation times again after the complete persisted-data read.
        for index in sorted({min(1, len(samples) - 1), len(samples) // 2, len(samples) - 1}):
            second, finish = samples[index]
            if second == 0: continue
            answer = api.request('POST', '/api/v2/query', {'version': 2, 'project': api.project,
                'time_range': {'end': timestamp(finish)}, 'signal': {'kind': 'metrics',
                    'expression': 'sum(rate(sift_acceptance_requests_' + run + '[5s]))'}}, 'metrics')
            require(bool(answer['data']['result']), 'historical metric rate returned no data')
        counts = dict(db.execute('SELECT signal, COUNT(*) FROM expected GROUP BY signal'))
        require(sum(counts.values()) == count, 'final readback count changed')
        return {'verified_items': count, 'signal_counts': counts, 'input_blocks': len(samples)}
    finally: db.close()
