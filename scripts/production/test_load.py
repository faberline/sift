"""Driver controls and a small public-API run. This is not a GKE load gate."""
import copy, http.client, json, os, socket, subprocess, tempfile, time, unittest, uuid
from pathlib import Path
from unittest.mock import patch
from load_data import build, compare, timestamp
from load_client import Api
from run_load import exercise

class Driver(unittest.TestCase):
    def test_every_item_has_an_identity_and_restarts_recreate_the_exact_input(self):
        first = build('a' * 32, 'prod', 120, 1_800_000_000_000_000_000, ['real body'])
        again = build('a' * 32, 'prod', 120, 1_800_000_000_000_000_000, ['real body'])
        self.assertEqual(first, again)
        self.assertEqual([len(first['expected'][name]) for name in ('logs', 'metrics', 'traces')], [8000, 1000, 1000])
        self.assertEqual(sum(batch[1] for batch in first['batches']), 10000)
        self.assertEqual(len({p['trace_id'] for p in first['expected']['traces'].values()}), 10)
        for point in first['expected']['metrics'].values():
            self.assertEqual(point['start_time_unix_nano'], first['start_nanos'])
    def test_missing_duplicate_wrong_resource_and_changed_bucket_data_fail(self):
        block = build('a' * 32, 'prod', 0, 1_800_000_000_000_000_000, ['body'], 10)
        identity, content = next(iter(block['expected']['logs'].items()))
        content = copy.deepcopy(content)
        for name in ('occurred_at', 'observed_at'): content[name] = timestamp(content[name])
        content['event_id'] = identity
        expected = {identity: block['expected']['logs'][identity]}
        compare(expected, [content], 'logs')
        bad = copy.deepcopy(content); bad['resource']['k8s.pod.uid'] = 'another-pod'
        for actual in ([], [content, content], [bad]):
            with self.assertRaises(ValueError): compare(expected, actual, 'logs')
        block = build('a' * 32, 'prod', 0, 1_800_000_000_000_000_000, ['body'], 100)
        identity, content = next((key, value) for key, value in block['expected']['metrics'].items() if 'histogram' in value)
        actual = dict(copy.deepcopy(content), event_id='stable-metric-id')
        compare({identity: content}, [actual], 'metrics')
        actual['histogram']['bucket_counts'][0] += 1
        with self.assertRaises(ValueError): compare({identity: content}, [actual], 'metrics')
    def test_remote_plain_http_and_token_in_url_are_refused(self):
        for endpoint in ('http://public.example.com', 'https://token@api.example.com', 'https://api.example.com/?secret=a'):
            with self.assertRaises(ValueError): Api(endpoint, 'p')
    def test_partial_success_is_not_counted_as_a_full_acknowledgement(self):
        api = Api('http://127.0.0.1:1', 'p')
        for response in [{'partialSuccess': {'rejectedLogRecords': '1'}},
                         {'partial_success': {'rejected_spans': 1}}, {'partialSuccess': {'errorMessage': 'unavailable'}}]:
            with patch.object(api, 'request', return_value=response):
                with self.assertRaises(ValueError): api.send(('logs', 1000, {}))
    def test_real_binary_writes_and_reads_every_item_without_creating_a_production_gate(self):
        repo = Path(__file__).resolve().parents[2]
        binary = Path(os.environ.get('SIFT_PRODUCTION_TEST_BIN', repo / 'target/debug/sift'))
        self.assertTrue(binary.is_file(), 'build the Sift binary before this test')
        with tempfile.TemporaryDirectory(prefix='sift-load-driver-') as directory:
            root = Path(directory)
            with socket.socket() as listener:
                listener.bind(('127.0.0.1', 0)); port = listener.getsockname()[1]
            env = {k: v for k, v in os.environ.items() if not k.startswith('SIFT_')}
            env['SIFT_AUTH'] = 'off'
            with (root / 'server.log').open('wb') as log:
                server = subprocess.Popen([str(binary), 'serve', '--host', '127.0.0.1', '--port', str(port),
                    '--data-dir', str(root / 'server')], env=env, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 20
                    while time.monotonic() < deadline:
                        self.assertIsNone(server.poll(), (root / 'server.log').read_text()[-4000:])
                        try:
                            connection = http.client.HTTPConnection('127.0.0.1', port, timeout=.5)
                            connection.request('GET', '/readyz'); response = connection.getresponse()
                            response.read(); connection.close()
                            if response.status == 200: break
                        except OSError: pass
                        time.sleep(.1)
                    else: self.fail('Sift did not become ready')
                    output = root / 'driver'; output.mkdir()
                    api = Api(f'http://127.0.0.1:{port}', 'local-driver')
                    result = exercise(api, ['sample 雨', 'second input'], output, 3, uuid.uuid4().hex, 'test', rate=100)
                    self.assertEqual(result['acknowledged_events'], 300)
                    self.assertEqual(len((output / 'verified.jsonl').read_text().splitlines()), 3)
                    self.assertEqual(list((output / 'pending').iterdir()), [])
                    self.assertFalse((output / 'load_30m.json').exists())
                finally:
                    if 'api' in locals(): api.close()
                    server.terminate()
                    try: server.wait(timeout=10)
                    except subprocess.TimeoutExpired: server.kill(); server.wait(timeout=10)

if __name__ == '__main__': unittest.main()
