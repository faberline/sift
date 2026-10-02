"""Offline controls for a bounded trial and its durable input records."""
import json, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
from load_client import Api
from load_data import encoded
from pilot_runtime import Receiver
from pilot_workflow import collected
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


if __name__ == '__main__': unittest.main()
