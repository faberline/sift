"""Tests for the one-process local load driver. Standard library only."""
import contextlib, http.server, io, json, os, shutil, sys, tempfile, threading, time, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'production'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from load_client import Api  # noqa: E402
import run_local_load as driver  # noqa: E402

STEP_KEYS = {
    'rate', 'seconds', 'offered_items', 'accepted_items', 'rejected_items', 'error_items',
    'ingest_latency_p50_seconds', 'ingest_latency_p95_seconds', 'log_query_p95_seconds',
    'metric_query_p95_seconds', 'query_errors', 'cpu_seconds', 'peak_rss_bytes',
    'disk_bytes_added', 'disk_bytes_per_accepted_item', 'queries_skipped',
}


class Fake:
    """A small local HTTP server that accepts OTLP posts and answers the query API."""

    def __init__(self):
        self.reject = False
        self.endless_logs = False
        self.query_calls = 0
        fake = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                length = int(self.headers.get('Content-Length', 0))
                data = self.rfile.read(length)
                if self.path in ('/v1/logs', '/v1/metrics', '/v1/traces'):
                    if fake.reject:
                        self.send_response(429)
                        self.send_header('Retry-After', '5')
                    else:
                        self.send_response(200)
                    body = b'{}'
                elif self.path == '/api/v1/query':
                    self.send_response(200)
                    if fake.endless_logs and b'"kind":"logs"' in data:
                        fake.query_calls += 1
                        body = json.dumps({'data': {'records': [], 'series': []},
                                           'next_cursor': str(fake.query_calls)}).encode()
                    else:
                        body = b'{"data":{"records":[],"series":[]}}'
                else:
                    self.send_response(404)
                    body = b'{}'
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.endpoint = 'http://127.0.0.1:%d' % self.server.server_address[1]

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class LocalLoad(unittest.TestCase):
    def test_percentile(self):
        self.assertIsNone(driver.percentile([]))
        self.assertEqual(driver.percentile(list(range(1, 101))), 95)

    def test_synthetic_templates(self):
        templates = driver.synthetic_templates()
        self.assertEqual(len(templates), 100)
        for text in templates:
            self.assertTrue(200 <= len(text.encode()) <= 600, len(text.encode()))
        self.assertEqual(len(set(templates)), 100)

    def test_templates_file_needs_100_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            short = Path(tmp) / 'short.txt'
            short.write_text('\n'.join('{"n":%d}' % i for i in range(99)) + '\n')
            with self.assertRaises(ValueError):
                driver.load_templates(short)
            full = Path(tmp) / 'full.txt'
            full.write_text('\n'.join('{"n":%d}' % i for i in range(100)) + '\n')
            self.assertEqual(len(driver.load_templates(full)), 100)

    def test_run_step_counts_accepted_items(self):
        fake = Fake()
        api = Api(fake.endpoint, 'p')
        try:
            with tempfile.TemporaryDirectory() as tmp:
                step = driver.run_step(api, 'r' * 32, 'e', 10, 2, driver.synthetic_templates(),
                                       time.time_ns(), None, Path(tmp))
        finally:
            api.close()
            fake.close()
        self.assertEqual(set(step), STEP_KEYS)
        self.assertEqual(step['offered_items'], 20)
        self.assertEqual(step['accepted_items'], 20)
        self.assertEqual(step['rejected_items'], 0)
        self.assertEqual(step['error_items'], 0)
        self.assertIsNone(step['cpu_seconds'])
        self.assertIsNone(step['peak_rss_bytes'])
        self.assertIsInstance(step['ingest_latency_p95_seconds'], float)

    def test_log_query_stops_at_page_cap(self):
        fake = Fake()
        fake.endless_logs = True
        api = Api(fake.endpoint, 'p')
        try:
            failed = driver._queries(api, 0, 10 ** 9, 'r')
        finally:
            api.close()
            fake.close()
        self.assertGreaterEqual(failed, 1)
        self.assertLessEqual(fake.query_calls, driver.MAX_LOG_PAGES + 1)

    def test_busy_query_worker_skips_queries(self):
        from unittest.mock import patch
        fake = Fake()
        api = Api(fake.endpoint, 'p')

        def slow_queries(api, start, end, run):
            time.sleep(7)
            return 0

        try:
            with tempfile.TemporaryDirectory() as tmp, \
                    patch.object(driver, '_queries', new=slow_queries):
                step = driver.run_step(api, 'r' * 32, 'e', 10, 10, driver.synthetic_templates(),
                                       time.time_ns(), None, Path(tmp))
        finally:
            api.close()
            fake.close()
        self.assertEqual(step['queries_skipped'], 1)

    def test_step_prints_phase_lines(self):
        fake = Fake()
        api = Api(fake.endpoint, 'p')
        out = io.StringIO()
        try:
            with tempfile.TemporaryDirectory() as tmp, contextlib.redirect_stdout(out):
                driver.run_step(api, 'r' * 32, 'e', 10, 2, driver.synthetic_templates(),
                                time.time_ns(), None, Path(tmp))
        finally:
            api.close()
            fake.close()
        text = out.getvalue()
        for line in ('step start rate=10', 'drain start rate=10', 'drain end rate=10', 'step end rate=10'):
            self.assertIn(line, text)

    def test_designed_run_time_fits_the_job_limit(self):
        self.assertEqual(driver.designed_seconds(5, 180), 1125)
        self.assertLess(driver.designed_seconds(5, 180), 45 * 60)

    def test_partial_result_says_it_is_not_complete(self):
        result = driver.build_result('sift', 180, Path('/nonexistent'), [], False, 'step 2 of 5 done')
        self.assertIs(result['complete'], False)
        self.assertEqual(result['last_phase'], 'step 2 of 5 done')
        self.assertIn('not a GKE claim', result['claim'])

    def test_over_budget_is_checked_before_each_step(self):
        self.assertTrue(driver.over_budget(2300, 180, 2400))
        self.assertFalse(driver.over_budget(0, 180, 2400))

    def test_rejection_is_counted_as_rejected_not_accepted(self):
        fake = Fake()
        fake.reject = True
        api = Api(fake.endpoint, 'p')
        try:
            with tempfile.TemporaryDirectory() as tmp:
                step = driver.run_step(api, 'r' * 32, 'e', 10, 2, driver.synthetic_templates(),
                                       time.time_ns(), None, Path(tmp))
        finally:
            api.close()
            fake.close()
        self.assertEqual(step['accepted_items'], 0)
        self.assertEqual(step['rejected_items'], 20)
        self.assertEqual(step['error_items'], 0)

    def test_disk_bytes_per_item(self):
        api = Api('http://127.0.0.1:1', 'p')

        def fake_send(self, batch):
            path = data_dir / 'written.bin'
            if not path.exists():
                path.write_bytes(b'x' * 1000)
            return batch[1]

        with tempfile.TemporaryDirectory() as tmp:
            data_dir = Path(tmp)
            from unittest.mock import patch
            with patch.object(Api, 'send', new=fake_send):
                step = driver.run_step(api, 'r' * 32, 'e', 10, 1, driver.synthetic_templates(),
                                       time.time_ns(), None, data_dir)
        self.assertEqual(step['disk_bytes_added'], 1000)
        self.assertEqual(step['disk_bytes_per_accepted_item'], 1000 / step['accepted_items'])

    def test_existing_output_dir_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            existing = Path(tmp) / 'exists'
            existing.mkdir()
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                code = driver.main(['--sift-bin', 'x', '--output-dir', str(existing)])
            self.assertEqual(code, 2)
            self.assertIn('output directory must be new', err.getvalue())
            self.assertEqual(list(existing.iterdir()), [])

    def test_rates_must_be_multiples_of_ten(self):
        with tempfile.TemporaryDirectory() as tmp:
            new = Path(tmp) / 'new'
            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    driver.main(['--sift-bin', 'x', '--output-dir', str(new), '--rates', '15'])
            self.assertFalse(new.exists())

    def test_rates_that_are_not_integers_are_a_usage_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            new = Path(tmp) / 'new'
            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    driver.main(['--sift-bin', 'x', '--output-dir', str(new), '--rates', 'abc'])
            self.assertFalse(new.exists())

    @unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('df'), 'GNU df on Linux')
    def test_machine_shape_reads_fs_type(self):
        with tempfile.TemporaryDirectory() as tmp:
            fs_type = driver.machine_shape(Path(tmp))['fs_type']
        self.assertIsInstance(fs_type, str)
        self.assertTrue(fs_type)

    @unittest.skipUnless(os.environ.get('SIFT_BIN'), 'set SIFT_BIN to the built sift binary')
    def test_real_binary_short_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'out'
            code = driver.main(['--sift-bin', os.environ['SIFT_BIN'], '--output-dir', str(out),
                                '--rates', '100', '--step-seconds', '5'])
            self.assertEqual(code, 0)
            text = (out / 'result.json').read_text()
            result = json.loads(text)
            self.assertEqual(len(result['steps']), 1)
            self.assertEqual(result['steps'][0]['accepted_items'], 500)
            self.assertIn('not a GKE claim', result['claim'])
            self.assertNotIn('/home', text)
            self.assertNotIn('/root', text)


if __name__ == '__main__':
    unittest.main()
