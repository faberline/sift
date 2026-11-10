"""Offline controls for a bounded trial and its durable input records."""
import http.client
import io
import json
import ssl
import subprocess
import tempfile
import unittest
from contextlib import ExitStack, redirect_stdout
from pathlib import Path
from unittest.mock import call, patch
from load_client import Api
from load_data import encoded, timestamp
from pilot_runtime import RECEIVER_LABEL, Receiver
from pilot_workflow import collected
from pilot_readback import readback
from run_pilot import input_block, limits, run


class Pilot(unittest.TestCase):
    def test_receiver_certificate_passes_strict_https_validation(self):
        with tempfile.TemporaryDirectory() as temp:
            receiver = Receiver(Path(temp) / 'receiver')
            receiver.start(port=0)
            context = ssl.create_default_context(cafile=str(receiver.cert))
            context.verify_flags |= ssl.VERIFY_X509_STRICT
            connection = http.client.HTTPSConnection(
                'localhost', receiver.server.server_port, timeout=3, context=context)
            try:
                connection.request('GET', '/readyz')
                response = connection.getresponse()
                self.assertEqual(response.status, 200)
                self.assertEqual(response.read(), b'')
            finally:
                connection.close()
                receiver.close()

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

    def test_docker_cleanup_retries_only_the_owned_id_and_verifies_absence(self):
        receiver = Receiver('unused')
        receiver.container = 'sift-pilot-receiver-test'
        identity = 'a' * 64
        present = f'{identity} {receiver.container}\n'
        remove = ['docker', 'rm', '--force', identity]
        with patch('pilot_runtime.subprocess.check_output', side_effect=[present, present, '']) as inventory, \
                patch('pilot_runtime.subprocess.run', side_effect=[subprocess.TimeoutExpired(remove, 15), None]) as command:
            receiver.close()
        self.assertIsNone(receiver.container)
        self.assertEqual(inventory.call_count, 3)
        for invoked in inventory.call_args_list:
            self.assertIn('label=' + RECEIVER_LABEL + '=sift-pilot-receiver-test', invoked.args[0])
        self.assertEqual(command.call_args_list, [
            call(remove, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15),
        ] * 2)

    def test_docker_cleanup_does_not_claim_success_if_owned_container_remains(self):
        receiver = Receiver('unused')
        receiver.container = 'sift-pilot-receiver-test'
        present = f'{"a" * 64} {receiver.container}\n'
        with patch('pilot_runtime.subprocess.check_output', return_value=present), \
                patch('pilot_runtime.subprocess.run') as command:
            with self.assertRaisesRegex(ValueError, 'still exists'):
                receiver.close()
        self.assertEqual(command.call_count, 2)
        self.assertEqual(receiver.container, 'sift-pilot-receiver-test')


class PilotCleanup(unittest.TestCase):
    def prepare(self, stack, temp, primary=None, cleanup_failures=None, receiver_start_error=None):
        binary, output = Path(temp) / 'sift', Path(temp) / 'run'
        binary.write_bytes(b'pilot test binary')
        stack.enter_context(patch('run_pilot.subprocess.check_output', return_value=json.dumps({
            'version': '0.2.0', 'git_sha': 'a' * 40})))
        receiver = stack.enter_context(patch('run_pilot.Receiver')).return_value
        receiver.start.side_effect = receiver_start_error
        service = stack.enter_context(patch('run_pilot.LocalSift')).return_value
        api = stack.enter_context(patch('run_pilot.Api')).return_value
        stack.enter_context(patch('run_pilot.setup', return_value={'expected': ['native log']}))
        stack.enter_context(patch('run_pilot.restart', return_value={}))
        stack.enter_context(patch('run_pilot.records', return_value=[]))
        stack.enter_context(patch('run_pilot.stream', side_effect=primary, return_value={'verified_items': 10}))
        stdout = stack.enter_context(redirect_stdout(io.StringIO()))
        cleaned = []
        def cleanup(name):
            def attempt():
                self.assertFalse((output / 'result.json').exists())
                cleaned.append(name)
                if cleanup_failures and name in cleanup_failures:
                    raise cleanup_failures[name]
            return attempt
        api.close.side_effect = cleanup('api')
        service.stop.side_effect = cleanup('service')
        receiver.close.side_effect = cleanup('receiver')
        return binary, output, cleaned, stdout

    def test_pass_is_published_only_after_all_three_cleanup_actions(self):
        with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
            binary, output, cleaned, stdout = self.prepare(stack, temp)
            result = run(binary, output, seconds=1, rate=10)
            self.assertEqual(cleaned, ['api', 'service', 'receiver'])
            self.assertEqual(result['result'], 'passed')
            self.assertEqual(json.loads((output / 'result.json').read_text()), result)
            self.assertEqual(json.loads(stdout.getvalue()), result)
            self.assertFalse((output / 'failure.json').exists())

    def test_receiver_cleanup_failure_cannot_publish_pass(self):
        with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
            binary, output, cleaned, stdout = self.prepare(stack, temp, cleanup_failures={
                'receiver': subprocess.TimeoutExpired(['docker', 'rm'], 15)})
            with self.assertRaisesRegex(ValueError, 'pilot cleanup failed'):
                run(binary, output, seconds=1, rate=10)
            self.assertEqual(cleaned, ['api', 'service', 'receiver'])
            self.assertFalse((output / 'result.json').exists())
            self.assertEqual(stdout.getvalue(), '')
            failure = json.loads((output / 'failure.json').read_text())
            self.assertEqual(failure['result'], 'failed')
            self.assertEqual(failure['cleanup_errors'][0]['resource'], 'receiver')
            self.assertEqual(failure['cleanup_errors'][0]['error_type'], 'TimeoutExpired')

    def test_cleanup_errors_do_not_mask_primary_failure_or_skip_later_cleanup(self):
        with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
            primary = ValueError('read-back failed: ' + 'x' * 600)
            binary, output, cleaned, stdout = self.prepare(stack, temp, primary, {
                'api': OSError('close failed'), 'receiver': subprocess.TimeoutExpired(['docker', 'rm'], 15)})
            with self.assertRaises(ValueError) as raised:
                run(binary, output, seconds=1, rate=10)
            self.assertIs(raised.exception, primary)
            self.assertEqual(cleaned, ['api', 'service', 'receiver'])
            self.assertFalse((output / 'result.json').exists())
            self.assertEqual(stdout.getvalue(), '')
            failure = json.loads((output / 'failure.json').read_text())
            self.assertEqual(failure['error_type'], 'ValueError')
            self.assertEqual(failure['error_message'], str(primary)[:512])
            self.assertEqual([entry['resource'] for entry in failure['cleanup_errors']], ['api', 'receiver'])

    def test_partially_started_receiver_is_still_cleaned_up(self):
        with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
            startup = subprocess.TimeoutExpired(['docker', 'run'], 30)
            binary, output, cleaned, _ = self.prepare(stack, temp, receiver_start_error=startup)
            with self.assertRaises(subprocess.TimeoutExpired) as raised:
                run(binary, output, seconds=1, rate=10)
            self.assertIs(raised.exception, startup)
            self.assertEqual(cleaned, ['receiver'])
            self.assertFalse((output / 'result.json').exists())
            self.assertEqual(json.loads((output / 'failure.json').read_text())['cleanup_errors'], [])


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
