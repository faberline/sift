"""Offline controls for a bounded trial and its durable input records."""
import json, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
from load_client import Api
from load_data import encoded, timestamp
from pilot_runtime import Receiver
from pilot_workflow import collected
from pilot_readback import readback
from run_pilot import input_block, limits


class Pilot(unittest.TestCase):
    def test_trial_cannot_use_a_rated_load_rate_or_an_unbounded_duration(self):
        for seconds, rate in ((0, 100), (1801, 100), (True, 100), (1800, 10_000), (1800, True), (1800, 99)):
            with self.subTest(seconds=seconds, rate=rate):
                with self.assertRaises(ValueError): limits(seconds, rate)
        limits(1800, 100)

    def test_persisted_inputs_match_regenerated_inputs_after_json_round_trip(self):
        before = input_block('a' * 32, 119, 1_800_000_000_000_000_000, 100, 'pilot-project')
        saved = json.loads(encoded(before))
        self.assertEqual(saved, input_block('a' * 32, 119, 1_800_000_000_000_000_000, 100, 'pilot-project'))
        saved['expected']['metrics'][next(iter(saved['expected']['metrics']))]['value'] += 1
        self.assertNotEqual(saved, before)

    def test_an_api_reused_after_restart_tracks_the_new_connection_for_next_cleanup(self):
        class Reply:
            status = 200
            def read(self, _): return b'{}'
        class Connection:
            def __init__(self, *_args, **_kwargs): self.closed = False
            def request(self, *_args): pass
            def getresponse(self): return Reply()
            def close(self): self.closed = True
        api = Api('http://127.0.0.1:1', 'p')
        with patch('load_client.http.client.HTTPConnection', Connection):
            api.request('GET', '/'); first = api.connections[0]; api.close()
            self.assertTrue(first.closed)
            api.request('GET', '/'); second = api.connections[0]
            self.assertIsNot(first, second)
            api.close(); self.assertTrue(second.closed)

    def test_a_partial_delivery_file_write_cannot_create_a_notification_record(self):
        with tempfile.TemporaryDirectory() as temp:
            receiver = Receiver.__new__(Receiver); receiver.root = Path(temp)
            (receiver.root / 'deliveries.jsonl').write_bytes(b'{"status":200}\n{"status":')
            self.assertEqual(receiver.snapshot(), [{'status': 200}])

    def test_native_collection_reads_all_pages_including_an_empty_filtered_page(self):
        def row(identity, raw):
            return {'event_id': identity, 'json_payload': {'raw': raw},
                'resource': {'service.name': 'sift-pilot'}}
        pages = [([row('first', 'a')], 'one'), ([], 'two'), ([row('last', 'b')], None)]
        class Pages:
            project = 'pilot-project'
            def request(self, _method, _path, request):
                self.cursor = request.get('cursor')
                records, cursor = pages.pop(0)
                return {'data': {'records': records}, 'next_cursor': cursor}
        api = Pages()
        self.assertEqual(set(collected(api, 'source', ['a', 'b'])), {'first', 'last'})
        self.assertEqual(api.cursor, 'two')
        self.assertFalse(pages)

    def test_a_repeated_native_log_cursor_cannot_claim_a_complete_read(self):
        class Pages:
            project = 'pilot-project'
            def request(self, *_):
                return {'data': {'records': []}, 'next_cursor': 'repeated'}
        with self.assertRaisesRegex(ValueError, 'cursor did not advance'):
            collected(Pages(), 'source', [])


class ReadbackApi:
    project = 'pilot-project'

    def __init__(self, blocks):
        self.rows = {signal: [] for signal in ('logs', 'metrics', 'traces')}
        for block in blocks:
            for signal, values in block['expected'].items():
                for key, value in values.items():
                    record = json.loads(encoded(value))
                    record['event_id'] = key if signal == 'logs' else signal + ':' + key
                    for field in ('occurred_at', 'observed_at'):
                        if field in record: record[field] = timestamp(record[field])
                    self.rows[signal].append(record)

    def log_pages(self, *_):
        yield []
        yield self.rows['logs'][:10]
        yield []
        yield self.rows['logs'][10:]

    def query(self, _signal, _start, _end, name):
        return [dict(record, points=[record]) for record in self.rows['metrics'] if record['name'] == name]

    def request(self, _method, path, *_args, **_kwargs):
        if path == '/api/v2/query': return {'data': {'result': [{}]}}
        trace_id = path.split('/')[4].split('?')[0]
        return {'spans': [row for row in self.rows['traces'] if row['trace_id'] == trace_id]}


class CompleteReadback(unittest.TestCase):
    def blocks(self):
        return [input_block('a' * 32, second, 1_800_000_000_000_000_000, 100, 'pilot-project') for second in range(2)]

    def check(self, api, blocks):
        with tempfile.TemporaryDirectory() as temp:
            return readback(api, iter(blocks), 'a' * 32, Path(temp) / 'expected.sqlite')

    def test_complete_readback_checks_every_signal_across_empty_log_pages(self):
        blocks = self.blocks()
        result = self.check(ReadbackApi(blocks), blocks)
        self.assertEqual(result, {'verified_items': 200, 'signal_counts': {'logs': 160, 'metrics': 20, 'traces': 20}, 'input_blocks': 2})

    def test_missing_duplicate_and_changed_records_cannot_pass(self):
        blocks = self.blocks()
        for fault in ('missing', 'duplicate', 'histogram', 'physical_identity'):
            with self.subTest(fault=fault):
                api = ReadbackApi(blocks)
                if fault == 'missing': api.rows['logs'].pop()
                elif fault == 'duplicate': api.rows['logs'].append(api.rows['logs'][0])
                elif fault == 'histogram':
                    next(row for row in api.rows['metrics'] if 'histogram' in row)['histogram']['bucket_counts'][0] += 1
                else: api.rows['metrics'][1]['event_id'] = api.rows['metrics'][0]['event_id']
                with self.assertRaises(ValueError): self.check(api, blocks)

    def test_log_page_reader_rejects_a_repeated_cursor(self):
        api = Api('http://127.0.0.1:1', 'p')
        api.request = lambda *_args: {'data': {'records': []}, 'next_cursor': 'repeated'}
        with self.assertRaisesRegex(ValueError, 'cursor did not advance'):
            list(api.log_pages(0, 1_000_000_000, 'run'))


if __name__ == '__main__': unittest.main()
