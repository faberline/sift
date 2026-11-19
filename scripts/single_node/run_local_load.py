"""One sift process (all roles, one data directory), load in rate steps, one result file.

This measures one machine. It is not a GKE or production claim.
"""
import argparse, concurrent.futures, datetime, http.client, json, math, os, socket, subprocess, sys, time, uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'production'))
from load_data import build  # noqa: E402
from load_client import Api  # noqa: E402

DEFAULT_RATES = '500,1000,2500,5000,10000'
QUERY_DEADLINE_SECONDS = 30
MAX_LOG_PAGES = 20
STARTUP_SECONDS = 30
SHUTDOWN_SECONDS = 20
JOB_LIMIT_SECONDS = 45 * 60
QUERY_ERRORS = (ValueError, OSError, http.client.HTTPException)


def say(message):
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%H:%M:%SZ')
    print(stamp + ' ' + message, flush=True)


def designed_seconds(step_count, step_seconds):
    return step_count * (step_seconds + QUERY_DEADLINE_SECONDS + 5) + STARTUP_SECONDS + SHUTDOWN_SECONDS


def designed_soak_seconds(soak_seconds, quiet_seconds):
    return (soak_seconds + QUERY_DEADLINE_SECONDS + 5 + quiet_seconds
            + STARTUP_SECONDS + SHUTDOWN_SECONDS)


def over_budget(elapsed, step_seconds, max_seconds):
    return elapsed + step_seconds + QUERY_DEADLINE_SECONDS + 5 > max_seconds


def percentile(values):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * .95) - 1]


def synthetic_templates():
    return ['{"level":"info","msg":"synthetic request %d","pad":"' % i + 'x' * (160 + (i * 4) % 300) + '"}'
            for i in range(100)]


def load_templates(path):
    if path is None:
        return synthetic_templates()
    lines = [line for line in path.read_text().splitlines() if line.strip()]
    if len(lines) < 100:
        raise ValueError('a templates file needs at least 100 non-empty lines')
    if any(len(line.encode()) > 8192 for line in lines):
        raise ValueError('a template line is over 8192 bytes')
    return lines


def machine_shape(data_dir):
    memory_bytes = None
    try:
        for line in Path('/proc/meminfo').read_text().splitlines():
            if line.startswith('MemTotal:'):
                memory_bytes = int(line.split()[1]) * 1024
                break
    except OSError:
        memory_bytes = None
    fs_type = None
    try:
        out = subprocess.run(['df', '--output=fstype', str(data_dir)], capture_output=True,
                             text=True, check=True).stdout.splitlines()
        fs_type = out[1].strip() if len(out) > 1 else None
    except (OSError, subprocess.CalledProcessError):
        fs_type = None
    return {'cpus': os.cpu_count(), 'memory_bytes': memory_bytes, 'fs_type': fs_type, 'platform': sys.platform}


def process_usage(pid):
    empty = {'cpu_seconds': None, 'peak_rss_bytes': None}
    if pid is None:
        return empty
    try:
        fields = Path('/proc/%d/stat' % pid).read_text().rsplit(')', 1)[1].split()
        # fields[0] is state (field 3); utime is field 14 and stime field 15.
        ticks = int(fields[11]) + int(fields[12])
        cpu_seconds = ticks / os.sysconf('SC_CLK_TCK')
        peak_rss_bytes = None
        for line in Path('/proc/%d/status' % pid).read_text().splitlines():
            if line.startswith('VmHWM:'):
                peak_rss_bytes = int(line.split()[1]) * 1024
        return {'cpu_seconds': cpu_seconds, 'peak_rss_bytes': peak_rss_bytes}
    except (OSError, IndexError, ValueError):
        return empty


def read_rss_bytes(pid, proc_root='/proc'):
    """The resident set size now (field VmRSS), or None when it cannot be read."""
    if pid is None:
        return None
    try:
        for line in (Path(proc_root) / str(pid) / 'status').read_text().splitlines():
            if line.startswith('VmRSS:'):
                return int(line.split()[1]) * 1024
    except (OSError, IndexError, ValueError):
        return None
    return None


class SoakSampler:
    """Collect [seconds since start, rss bytes] about every `sample_seconds`."""

    def __init__(self, read_rss, sample_seconds):
        self.read_rss = read_rss
        self.sample_seconds = sample_seconds
        self.next_due = sample_seconds
        self.samples = []

    def tick(self, elapsed_seconds):
        if elapsed_seconds < self.next_due:
            return
        self.next_due += self.sample_seconds
        rss = self.read_rss()
        if rss is not None:
            self.samples.append([elapsed_seconds, rss])


def quiet_rss(pid, quiet_seconds, read_rss=None, sleep=time.sleep):
    """Wait `quiet_seconds` with no load, then read the resident set size once."""
    sleep(quiet_seconds)
    return (read_rss or read_rss_bytes)(pid)


def directory_bytes(path):
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += os.stat(os.path.join(root, name)).st_size
            except FileNotFoundError:
                continue
    return total


def run_step(api, run, environment, rate, seconds, templates, base_nanos, pid, data_dir,
             on_second=None):
    api.timings['logs'].clear()
    api.timings['metrics'].clear()
    before = directory_bytes(data_dir)
    cpu0 = process_usage(pid)['cpu_seconds']
    start = time.monotonic()
    accepted = rejected = errored = query_errors = queries_skipped = 0
    send_times = []
    query_future = None
    say('step start rate=%d seconds=%d' % (rate, seconds))

    def send_one(batch):
        began = time.monotonic()
        try:
            api.send(batch)
            status = 'ok'
        except ValueError as exc:
            text = str(exc)
            status = 'rejected' if ('HTTP 429' in text or 'HTTP 503' in text) else 'error'
        except Exception:
            status = 'error'
        return status, batch[1], time.monotonic() - began

    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as query_pool, \
            concurrent.futures.ThreadPoolExecutor(max_workers=12) as send_pool:
        for second in range(seconds):
            delay = start + second - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            block = build(run, environment, second, base_nanos, templates, rate)
            futures = [send_pool.submit(send_one, batch) for batch in block['batches']]
            for future in futures:
                status, count, elapsed = future.result()
                send_times.append(elapsed)
                if status == 'ok':
                    accepted += count
                elif status == 'rejected':
                    rejected += count
                else:
                    errored += count
            if on_second is not None:
                on_second(second)
            if second % 5 == 4:
                if query_future is None or query_future.done():
                    if query_future is not None:
                        query_errors += query_future.result()
                    query_future = query_pool.submit(
                        _queries, api, block['start_nanos'], block['end_nanos'], run)
                else:
                    queries_skipped += 1
        say('drain start rate=%d' % rate)
        if query_future is not None:
            query_errors += query_future.result()
        say('drain end rate=%d' % rate)

    after = directory_bytes(data_dir)
    usage = process_usage(pid)
    cpu_seconds = None
    if cpu0 is not None and usage['cpu_seconds'] is not None:
        cpu_seconds = usage['cpu_seconds'] - cpu0
    ordered = sorted(send_times)
    p50 = ordered[len(ordered) // 2] if ordered else None
    say('step end rate=%d accepted=%d rejected=%d errors=%d' % (rate, accepted, rejected, errored))
    return {
        'rate': rate,
        'seconds': seconds,
        'offered_items': rate * seconds,
        'accepted_items': accepted,
        'rejected_items': rejected,
        'error_items': errored,
        'ingest_latency_p50_seconds': p50,
        'ingest_latency_p95_seconds': percentile(send_times),
        'log_query_p95_seconds': percentile(api.timings['logs']),
        'metric_query_p95_seconds': percentile(api.timings['metrics']),
        'query_errors': query_errors,
        'queries_skipped': queries_skipped,
        'cpu_seconds': cpu_seconds,
        'peak_rss_bytes': usage['peak_rss_bytes'],
        'disk_bytes_added': after - before,
        'disk_bytes_per_accepted_item': None if accepted == 0 else (after - before) / accepted,
    }


def _queries(api, start, end, run):
    """Run one log query and one metric query for a second; return how many failed."""
    failed = 0
    deadline = time.monotonic() + QUERY_DEADLINE_SECONDS
    try:
        pages = 0
        for _page in api.log_pages(start, end, run):
            pages += 1
            if pages >= MAX_LOG_PAGES or time.monotonic() > deadline:
                failed += 1
                break
    except QUERY_ERRORS:
        failed += 1
    try:
        api.query('metrics', start, end, name='sift_acceptance_requests_' + run)
    except QUERY_ERRORS:
        failed += 1
    return failed


def start_server(sift_bin, data_dir, log_path):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    env = {key: value for key, value in os.environ.items() if not key.startswith('SIFT_')}
    env['SIFT_AUTH'] = 'off'
    with open(log_path, 'wb') as log:
        process = subprocess.Popen([str(sift_bin), 'serve', '--host', '127.0.0.1', '--port', str(port),
                                    '--data-dir', str(data_dir)], env=env, stdout=log, stderr=log)
    deadline = time.monotonic() + 30
    while True:
        if process.poll() is not None:
            raise RuntimeError('sift serve exited before it was ready')
        try:
            connection = http.client.HTTPConnection('127.0.0.1', port, timeout=2)
            connection.request('GET', '/readyz')
            ready = connection.getresponse().status == 200
            connection.close()
            if ready:
                return process, port
        except OSError:
            pass
        if time.monotonic() > deadline:
            process.kill()
            raise RuntimeError('sift serve was not ready within 30 seconds')
        time.sleep(0.2)


def build_result(sift_bin_name, step_seconds, data_dir, steps, complete, last_phase, soak=None):
    result = {
        'kind': 'single-node-local-load',
        'claim': 'measured on this machine, not a GKE claim',
        'machine': machine_shape(data_dir),
        'sift_bin': sift_bin_name,
        'step_seconds': step_seconds,
        'steps': steps,
        'complete': complete,
        'last_phase': last_phase,
    }
    if soak is not None:
        result.update(soak)
    return result


def _rates(text):
    rates = [int(part) for part in text.split(',') if part.strip()]
    return rates


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sift-bin', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--rates', default=DEFAULT_RATES)
    parser.add_argument('--step-seconds', type=int, default=180)
    parser.add_argument('--templates', type=Path)
    parser.add_argument('--max-seconds', type=int, default=2400)
    parser.add_argument('--soak-rate', type=int, default=None)
    parser.add_argument('--soak-seconds', type=int, default=600)
    parser.add_argument('--sample-seconds', type=int, default=30)
    parser.add_argument('--quiet-seconds', type=int, default=120)
    args = parser.parse_args(argv)

    try:
        rates = _rates(args.rates)
    except ValueError:
        parser.error('each rate must be a multiple of 10 from 10 through 10000')
    if not rates or any(rate % 10 or rate < 10 or rate > 10000 for rate in rates):
        parser.error('each rate must be a multiple of 10 from 10 through 10000')
    if args.step_seconds < 1:
        parser.error('--step-seconds must be at least 1')
    step_seconds = args.step_seconds
    max_seconds = args.max_seconds
    soak = args.soak_rate is not None
    if soak:
        if args.soak_rate % 10 or args.soak_rate < 10 or args.soak_rate > 10000:
            parser.error('--soak-rate must be a multiple of 10 from 10 through 10000')
        if args.soak_seconds < 1 or args.sample_seconds < 1 or args.quiet_seconds < 0:
            parser.error('--soak-seconds and --sample-seconds must be at least 1 '
                         'and --quiet-seconds at least 0')
        if designed_soak_seconds(args.soak_seconds, args.quiet_seconds) >= JOB_LIMIT_SECONDS:
            parser.error('the soak run does not fit the 45 minute job limit')
        rates = [args.soak_rate]
        step_seconds = args.soak_seconds
        max_seconds = JOB_LIMIT_SECONDS

    if args.output_dir.exists():
        print('output directory must be new', file=sys.stderr)
        return 2
    try:
        templates = load_templates(args.templates)
    except (OSError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 2

    args.output_dir.mkdir(mode=0o700, parents=True)
    data_dir = args.output_dir / 'data'
    log_path = args.output_dir / 'server.log'
    process = None
    api = None
    steps = []
    stopped_phase = 'no step done'
    started = time.monotonic()
    sampler = None
    rss_after_quiet = None

    def soak_fields():
        if not soak:
            return None
        return {'soak_rate': args.soak_rate, 'soak_seconds': args.soak_seconds,
                'sample_seconds': args.sample_seconds, 'quiet_seconds': args.quiet_seconds,
                'rss_samples': sampler.samples if sampler else [],
                'rss_after_quiet_bytes': rss_after_quiet}

    try:
        say('server start')
        process, port = start_server(args.sift_bin, data_dir, log_path)
        say('server ready port=%d' % port)
        if soak:
            sampler = SoakSampler(lambda: read_rss_bytes(process.pid), args.sample_seconds)
        for rate in rates:
            if over_budget(time.monotonic() - started, step_seconds, max_seconds):
                say('stop before rate=%d: time budget' % rate)
                stopped_phase = 'stopped before rate %d: time budget' % rate
                break
            run = uuid.uuid4().hex
            base_nanos = time.time_ns()
            api = Api('http://127.0.0.1:%d' % port, 'single-node-load')
            try:
                extra = {}
                if soak:
                    extra['on_second'] = lambda second: sampler.tick(second + 1)
                steps.append(run_step(api, run, 'single-node-load', rate, step_seconds,
                                      templates, base_nanos, process.pid, data_dir, **extra))
            except Exception as exc:
                say('stop at rate=%d: %s' % (rate, type(exc).__name__))
                stopped_phase = 'stopped at rate %d: %s' % (rate, type(exc).__name__)
                break
            finally:
                api.close()
                api = None
            partial = build_result(args.sift_bin.name, step_seconds, data_dir, steps, False,
                                   'step %d of %d done' % (len(steps), len(rates)),
                                   soak=soak_fields())
            (args.output_dir / 'result.json').write_text(json.dumps(partial, indent=2) + '\n')
        if soak and len(steps) == len(rates):
            say('quiet time %d seconds' % args.quiet_seconds)
            rss_after_quiet = quiet_rss(process.pid, args.quiet_seconds)
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        if api is not None:
            api.close()

    complete = len(steps) == len(rates)
    result = build_result(args.sift_bin.name, step_seconds, data_dir, steps, complete,
                          'all steps done' if complete else stopped_phase, soak=soak_fields())
    path = args.output_dir / 'result.json'
    path.write_text(json.dumps(result, indent=2) + '\n')
    say('result written')
    print(path)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
