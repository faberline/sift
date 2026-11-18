import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / '.github' / 'workflows' / 'sift-single-node-load.yml'
RESULT = ROOT / 'docs' / 'operations' / 'single-node-capacity-result.json'
TEXT = PATH.read_text()

STEP_KEYS = {
    'rate', 'seconds', 'offered_items', 'accepted_items', 'rejected_items', 'error_items',
    'ingest_latency_p50_seconds', 'ingest_latency_p95_seconds', 'log_query_p95_seconds',
    'metric_query_p95_seconds', 'query_errors', 'cpu_seconds', 'peak_rss_bytes',
    'disk_bytes_added', 'disk_bytes_per_accepted_item',
}


def on_block():
    lines = TEXT.splitlines()
    start = next(i for i, line in enumerate(lines) if line == 'on:')
    block = []
    for line in lines[start + 1:]:
        if line and not line.startswith(' '):
            break
        block.append(line)
    return '\n'.join(block)


class WorkflowTest(unittest.TestCase):
    def test_trigger_is_only_workflow_dispatch(self):
        keys = re.findall(r'^  ([A-Za-z_]+):', on_block(), re.M)
        self.assertEqual(keys, ['workflow_dispatch'])

    def test_no_pull_request_or_schedule_trigger(self):
        block = on_block()
        for word in ('pull_request', 'pull_request_target', 'push:', 'schedule', 'workflow_run'):
            self.assertNotIn(word, block)

    def test_workflow_name_is_not_gate(self):
        match = re.search(r'^name:\s*(.+)$', TEXT, re.M)
        self.assertIsNotNone(match)
        self.assertEqual(match.group(1).strip().strip('"\''), 'sift-single-node-load')
        self.assertNotEqual(match.group(1).strip(), 'gate')
        self.assertIsNone(re.search(r'^  gate:', TEXT, re.M))

    def test_job_limits(self):
        self.assertIn('runs-on: ubuntu-latest', TEXT)
        self.assertIn('timeout-minutes: 45', TEXT)
        self.assertIn('permissions:', TEXT)
        self.assertIn('contents: read', TEXT)
        self.assertNotIn('secrets.', TEXT)
        self.assertNotIn('id-token', TEXT)

    def test_runs_driver_and_uploads_result(self):
        for text in ('cargo build --release', '--bin sift',
                     'scripts/single_node/run_local_load.py', 'result.json',
                     'actions/upload-artifact@'):
            self.assertIn(text, TEXT)

    def test_committed_result_has_every_default_step(self):
        data = json.loads(RESULT.read_text())
        self.assertEqual(data['kind'], 'single-node-local-load')
        self.assertIn('not a GKE claim', data['claim'])
        self.assertEqual(data['step_seconds'], 60)
        self.assertEqual([step['rate'] for step in data['steps']], [500, 1000, 2500, 5000, 10000])
        self.assertIsInstance(data['machine']['cpus'], int)
        self.assertIsInstance(data['machine']['memory_bytes'], int)
        for step in data['steps']:
            self.assertEqual(set(step), STEP_KEYS)


if __name__ == '__main__':
    unittest.main()
