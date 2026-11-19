import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / '.github' / 'workflows' / 'sift-single-node-load.yml'
TEXT = PATH.read_text()


def on_block():
    lines = TEXT.splitlines()
    start = next(i for i, line in enumerate(lines) if line == 'on:')
    block = []
    for line in lines[start + 1:]:
        if line and not line.startswith(' '):
            break
        block.append(line)
    return '\n'.join(block)


def step_block(marker):
    blocks = re.split(r'(?m)^(?=      - )', TEXT)
    return next(block for block in blocks if marker in block)


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

    def test_logs_are_uploaded_even_when_the_driver_fails(self):
        self.assertIn('        if: always()', step_block('name: single-node-load-logs'))

    def test_server_log_and_driver_log_are_collected(self):
        for text in ('driver.log', 'server.log', 'tee'):
            self.assertIn(text, TEXT)

    def test_result_upload_runs_only_after_success(self):
        block = step_block('name: single-node-load-result')
        self.assertNotIn('always()', block)
        self.assertNotIn('failure()', block)

    def test_data_directory_is_not_uploaded(self):
        self.assertNotIn('single-node-load/data', TEXT)
        self.assertIsNone(re.search(r'path:.*data', TEXT))

    def test_driver_output_is_unbuffered(self):
        self.assertIn('python3 -u', TEXT)

    def test_driver_log_is_kept_when_the_driver_fails(self):
        self.assertIn('set -o pipefail', step_block('name: Run the local load driver'))

    def test_no_result_file_is_committed(self):
        self.assertFalse((ROOT / 'docs' / 'operations' / 'single-node-capacity-result.json').exists())


if __name__ == '__main__':
    unittest.main()
