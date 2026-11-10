"""Compare complete canonical exports against generated input, GCP, or pre-fault Sift data."""
import argparse, hashlib, json, os, re, sqlite3, tempfile
from pathlib import Path

SIGNALS = ('logs', 'metrics', 'traces', 'alerts', 'control')
HEAD = {'schema','origin','source_commit','image','cluster_identity_sha256','start_unix_nano',
        'end_unix_nano','services','complete','sampled','truncated','record_counts'}

def require(condition, message):
    if not condition: raise ValueError(message)

def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, allow_nan=False, separators=(',', ':')).encode()

def parse(raw):
    def no_duplicate(items):
        result = {}
        for key,value in items:
            require(key not in result, 'duplicate JSON key'); result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=no_duplicate,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite JSON number')))

def import_file(db, path, table, maximum):
    require(path.is_file() and not path.is_symlink(), 'export must be a regular file')
    require(0 < path.stat().st_size <= 8 * 1024**3, 'export exceeds its byte limit')
    counts, sha = {signal: 0 for signal in SIGNALS}, hashlib.sha256()
    with path.open('rb') as inp:
        first = inp.readline(1_048_577)
        require(0 < len(first) <= 1_048_576, 'export header exceeds its limit')
        sha.update(first); header = parse(first)
        require(set(header) == HEAD and header['schema'] == 'sift.production-observations/v1', 'export header differs')
        require(header['complete'] is True and header['sampled'] is False and header['truncated'] is False,
                'partial or sampled exports cannot prove correctness')
        require(header['origin'] in ('sift', 'gcp', 'generated'), 'export origin differs')
        require(type(header['source_commit']) is str and re.fullmatch('[0-9a-f]{40}',header['source_commit']), 'candidate source is invalid')
        require(type(header['image']) is str and re.fullmatch(r'ghcr\.io/faberline/sift@sha256:[0-9a-f]{64}',header['image']), 'candidate image is invalid')
        require(type(header['cluster_identity_sha256']) is str and re.fullmatch('[0-9a-f]{64}',header['cluster_identity_sha256']), 'cluster identity is invalid')
        require(type(header['start_unix_nano']) is int and type(header['end_unix_nano']) is int
                and 0 <= header['start_unix_nano'] < header['end_unix_nano'], 'export time window differs')
        require(type(header['services']) is list and header['services'], 'source services are missing')
        require(type(header['record_counts']) is dict and set(header['record_counts']) == set(SIGNALS), 'signal counts differ')
        require(all(type(n) is int and n >= 0 for n in header['record_counts'].values()), 'invalid signal count')
        require(sum(header['record_counts'].values()) <= maximum, 'export exceeds its row limit')
        for index in range(maximum + 1):
            raw = inp.readline(1_048_577)
            if not raw: break
            require(index < maximum and len(raw) <= 1_048_576, 'export row or byte limit exceeded')
            sha.update(raw); row = parse(raw)
            require(set(row) == {'signal', 'identity', 'content'} and row['signal'] in SIGNALS, 'export row differs')
            require(type(row['identity']) is dict and type(row['content']) is dict and row['content'], 'export content is missing')
            for name in ('project','environment'):
                require(type(row['identity'].get(name)) is str and row['identity'][name], 'tenant identity is missing')
            require(len(row['identity']) > 2, 'source identity is missing')
            key = canonical(row['identity']); content = canonical(row['content'])
            try:
                db.execute('INSERT INTO ' + table + ' VALUES(?,?,?)',
                    (row['signal'], key, hashlib.sha256(content).digest()))
            except sqlite3.IntegrityError as error: raise ValueError('duplicate source identity') from error
            counts[row['signal']] += 1
        require(counts == header['record_counts'], 'export counts do not match all rows')
    db.commit()
    return header, sha.hexdigest(), counts

def compare(before, after, mode, maximum=2_000_000):
    require(mode in ('recovery','dual-write','generated'), 'comparison mode differs')
    with tempfile.TemporaryDirectory(prefix='sift-export-comparison-') as temp:
        db = sqlite3.connect(str(Path(temp) / 'observations.sqlite'))
        try:
            db.execute('PRAGMA journal_mode=OFF')
            db.execute('PRAGMA cache_size=-16384')
            db.execute('PRAGMA temp_store=FILE')
            db.execute('PRAGMA max_page_count=1572864')  # Six GiB maximum at the default page size.
            for table in ('before_rows','after_rows'):
                db.execute('CREATE TABLE '+table+'(signal TEXT,key BLOB,sha BLOB,PRIMARY KEY(signal,key)) WITHOUT ROWID')
            bh, bs, bc = import_file(db, Path(before), 'before_rows', maximum)
            ah, ass, ac = import_file(db, Path(after), 'after_rows', maximum)
            origin={'recovery':'sift','dual-write':'gcp','generated':'generated'}[mode]
            require(bh['origin']==origin and ah['origin']=='sift','comparison origins differ')
            for name in ('source_commit','image','cluster_identity_sha256','start_unix_nano','end_unix_nano','services'):
                require(bh[name] == ah[name], 'comparison ' + name + ' differs')
            require(sum(bc.values()) > 0, 'empty exports cannot prove recovery or comparison')
            differences = {}
            for signal in SIGNALS:
                missing = db.execute('SELECT count(*) FROM before_rows b LEFT JOIN after_rows a '
                    'ON b.signal=a.signal AND b.key=a.key WHERE b.signal=? AND a.key IS NULL', (signal,)).fetchone()[0]
                extra = db.execute('SELECT count(*) FROM after_rows a LEFT JOIN before_rows b '
                    'ON b.signal=a.signal AND b.key=a.key WHERE a.signal=? AND b.key IS NULL', (signal,)).fetchone()[0]
                changed = db.execute('SELECT count(*) FROM before_rows b JOIN after_rows a '
                    'ON b.signal=a.signal AND b.key=a.key WHERE b.signal=? AND b.sha<>a.sha', (signal,)).fetchone()[0]
                differences[signal] = {'missing':missing,'extra':extra,'changed':changed}
            return {'schema':'sift.production-comparison/v2','result':'matched' if not any(
                any(value.values()) for value in differences.values()) else 'different', 'production_accepted':False,
                'reference_origin':bh['origin'],
                'source_commit':bh['source_commit'],'image':bh['image'],'cluster_identity_sha256':bh['cluster_identity_sha256'],
                'start_unix_nano':bh['start_unix_nano'],'end_unix_nano':bh['end_unix_nano'],
                'before_sha256':bs,'after_sha256':ass,'compared_records':bc,'differences':differences}
        finally: db.close()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before',type=Path,required=True); parser.add_argument('--after',type=Path,required=True)
    parser.add_argument('--mode',choices=['recovery','dual-write','generated'],required=True)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    result = compare(args.before,args.after,args.mode)
    with args.output.open('x') as out:
        os.chmod(args.output,0o600); json.dump(result,out,indent=2); out.write('\n')
    require(result['result']=='matched','source content or identity differs; stop cutover')
    print('Complete exports match. Live duration, export coverage, and other gates remain required.')

if __name__ == '__main__':
    try: main()
    except (ValueError,KeyError,TypeError,OSError,sqlite3.Error) as error: raise SystemExit('comparison refused: '+str(error))
