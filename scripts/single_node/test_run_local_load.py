"""Tests for the one-process local load driver. Standard library only."""
import contextlib, http.server, io, json, os, shutil, sys, tempfile, threading, time, unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'production'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from load_client import Api  # noqa: E402
from load_data import build  # noqa: E402
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


class TimeoutApi(Api):
    """An Api whose log query raises a socket timeout, as a stalled server does."""
    def log_pages(self, start, end, run):
        raise TimeoutError('timed out')
        yield  # makes this a generator, like Api.log_pages


class LocalLoad(unittest.TestCase):
    def test_percentile(self):
        self.assertIsNone(driver.percentile([]))
        self.assertEqual(driver.percentile(list(range(1, 101))), 95)

    def test_query_window_is_one_second_and_never_overlaps(self):
        first = build('run', 'env', 0, 1_000_000_000_000, ['body'], 2500)
        later = build('run', 'env', 7, 1_000_000_000_000, ['body'], 2500)
        for block in (first, later):
            self.assertEqual(block['end_nanos'] - block['start_nanos'], 1_000_000_000)
        self.assertGreaterEqual(later['start_nanos'], first['end_nanos'])
        self.assertEqual(later['start_nanos'] - first['start_nanos'], 7_000_000_000)

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
        self.assertLessEqual(fake.query_calls, driver.MAX_LOG_PAGES)

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

    def test_max_seconds_stops_before_the_next_step(self):
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

    def test_query_timeout_counts_as_query_error(self):
        fake = Fake()
        api = TimeoutApi(fake.endpoint, 'p')
        try:
            failed = driver._queries(api, 0, 10 ** 9, 'r')
        finally:
            api.close()
            fake.close()
        self.assertEqual(failed, 1)

    def test_step_with_query_timeout_still_returns_a_result(self):
        fake = Fake()
        api = TimeoutApi(fake.endpoint, 'p')
        try:
            with tempfile.TemporaryDirectory() as tmp:
                step = driver.run_step(api, 'r' * 32, 'e', 10, 5, driver.synthetic_templates(),
                                       time.time_ns(), None, Path(tmp))
        finally:
            api.close()
            fake.close()
        self.assertGreaterEqual(step['query_errors'], 1)
        self.assertEqual(step['accepted_items'], 50)
        self.assertEqual(set(step), STEP_KEYS)

    def test_failed_step_keeps_finished_steps_in_the_result(self):
        class StoppedProcess:
            pid = None

            def terminate(self):
                pass

            def wait(self, timeout=None):
                pass

        def fake_run_step(api, run, environment, rate, *args):
            if rate == 100:
                return {'rate': rate}
            raise TimeoutError('timed out')

        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'out'
            with mock.patch.object(driver, 'start_server', return_value=(StoppedProcess(), 1234)), \
                    mock.patch.object(driver, 'run_step', side_effect=fake_run_step), \
                    contextlib.redirect_stdout(io.StringIO()):
                code = driver.main(['--sift-bin', 'x', '--output-dir', str(out),
                                    '--rates', '100,200', '--step-seconds', '1'])
            self.assertEqual(code, 0)
            result = json.loads((out / 'result.json').read_text())
        self.assertEqual(result['steps'], [{'rate': 100}])
        self.assertIs(result['complete'], False)
        self.assertEqual(result['last_phase'], 'stopped at rate 200: TimeoutError')

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


class StoppedProcess:
    pid = None

    def terminate(self):
        pass

    def wait(self, timeout=None):
        pass


SOAK_KEYS = {'soak_rate', 'soak_seconds', 'sample_seconds', 'quiet_seconds',
             'rss_samples', 'rss_after_quiet_bytes'}


class SoakMode(unittest.TestCase):
    def run_main(self, argv, fake_run_step):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'out'
            with mock.patch.object(driver, 'start_server', return_value=(StoppedProcess(), 1234)), \
                    mock.patch.object(driver, 'run_step', side_effect=fake_run_step) as run_step, \
                    mock.patch.object(driver, 'read_rss_bytes', return_value=777), \
                    contextlib.redirect_stdout(io.StringIO()):
                code = driver.main(['--sift-bin', 'x', '--output-dir', str(out)] + argv)
            self.assertEqual(code, 0)
            return json.loads((out / 'result.json').read_text()), run_step

    def test_read_rss_bytes_reads_vmrss(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / '42').mkdir()
            (Path(tmp) / '42' / 'status').write_text('Name:\tsift\nVmRSS:\t  2048 kB\n')
            self.assertEqual(driver.read_rss_bytes(42, tmp), 2048 * 1024)
            self.assertIsNone(driver.read_rss_bytes(43, tmp))
        self.assertIsNone(driver.read_rss_bytes(None))

    def test_sampler_records_about_every_sample_seconds(self):
        values = iter([1, 2, 3, 4])
        sampler = driver.SoakSampler(lambda: next(values), 30)
        for elapsed in range(1, 96):
            sampler.tick(elapsed)
        self.assertEqual(sampler.samples, [[30, 1], [60, 2], [90, 3]])

    def test_sampler_skips_an_unreadable_value(self):
        sampler = driver.SoakSampler(lambda: None, 30)
        for elapsed in range(1, 61):
            sampler.tick(elapsed)
        self.assertEqual(sampler.samples, [])

    def test_quiet_rss_waits_before_it_reads(self):
        events = []
        value = driver.quiet_rss(7, 120, read_rss=lambda pid: events.append(('read', pid)) or 99,
                                 sleep=lambda seconds: events.append(('sleep', seconds)))
        self.assertEqual(value, 99)
        self.assertEqual(events, [('sleep', 120), ('read', 7)])

    def test_designed_soak_time(self):
        self.assertEqual(driver.designed_soak_seconds(1800, 120), 2005)
        self.assertLess(driver.designed_soak_seconds(1800, 120), driver.JOB_LIMIT_SECONDS)
        self.assertGreaterEqual(driver.designed_soak_seconds(2600, 120), driver.JOB_LIMIT_SECONDS)

    def test_build_result_without_soak_has_no_soak_keys(self):
        result = driver.build_result('sift', 180, Path('/nonexistent'), [], True, 'all steps done')
        self.assertTrue(SOAK_KEYS.isdisjoint(result))

    def test_build_result_with_soak_has_the_soak_keys(self):
        soak = {'soak_rate': 100, 'soak_seconds': 60, 'sample_seconds': 30, 'quiet_seconds': 0,
                'rss_samples': [[30, 1]], 'rss_after_quiet_bytes': 2}
        result = driver.build_result('sift', 60, Path('/nonexistent'), [], True, 'x', soak=soak)
        self.assertTrue(SOAK_KEYS <= set(result))
        self.assertEqual(result['rss_samples'], [[30, 1]])

    def test_run_step_calls_on_second_for_each_second(self):
        fake = Fake()
        api = Api(fake.endpoint, 'p')
        seen = []
        try:
            with tempfile.TemporaryDirectory() as tmp:
                driver.run_step(api, 'r' * 32, 'e', 10, 3, driver.synthetic_templates(),
                                time.time_ns(), None, Path(tmp), on_second=seen.append)
        finally:
            api.close()
            fake.close()
        self.assertEqual(seen, [0, 1, 2])

    def test_main_in_soak_mode_writes_samples_and_the_quiet_value(self):
        def fake_run_step(api, run, environment, rate, seconds, templates, base_nanos, pid,
                          data_dir, on_second=None):
            for second in range(seconds):
                on_second(second)
            return {'rate': rate, 'seconds': seconds}

        result, _run_step = self.run_main(
            ['--soak-rate', '100', '--soak-seconds', '60', '--sample-seconds', '30',
             '--quiet-seconds', '0'], fake_run_step)
        self.assertEqual(result['rss_samples'], [[30, 777], [60, 777]])
        self.assertEqual(result['rss_after_quiet_bytes'], 777)
        self.assertEqual(result['soak_rate'], 100)
        self.assertEqual(result['steps'], [{'rate': 100, 'seconds': 60}])
        self.assertIs(result['complete'], True)

    def test_main_without_soak_rate_has_no_rss_samples(self):
        result, run_step = self.run_main(
            ['--rates', '100', '--step-seconds', '1'],
            lambda api, run, environment, rate, *args, **kwargs: {'rate': rate})
        self.assertTrue(SOAK_KEYS.isdisjoint(result))
        self.assertNotIn('on_second', run_step.call_args.kwargs)

    def test_soak_run_longer_than_the_job_limit_is_a_usage_error(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as raised:
                driver.main(['--sift-bin', 'x', '--output-dir', str(Path(tmp) / 'out'),
                             '--soak-rate', '100', '--soak-seconds', '2600'])
        self.assertEqual(raised.exception.code, 2)

    def test_soak_rate_must_be_a_multiple_of_ten(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as raised:
                driver.main(['--sift-bin', 'x', '--output-dir', str(Path(tmp) / 'out'),
                             '--soak-rate', '15'])
        self.assertEqual(raised.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
