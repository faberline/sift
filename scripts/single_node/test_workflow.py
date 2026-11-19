import json
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

    def test_result_file_is_committed_and_complete(self):
        path = ROOT / 'docs' / 'operations' / 'single-node-capacity-result.json'
        self.assertTrue(path.exists())
        self.assertIs(json.loads(path.read_text(encoding='utf-8'))['complete'], True)

    def test_soak_inputs_exist_and_default_to_empty(self):
        block = on_block()
        for name in ('soak_rate', 'soak_seconds'):
            match = re.search(r'(?m)^      %s:\n((?:        .*\n)+)' % name, block + '\n')
            self.assertIsNotNone(match, name)
            self.assertIn('default: ""', match.group(1))
            self.assertIn('type: string', match.group(1))

    def test_soak_inputs_reach_the_script_only_through_env(self):
        self.assertEqual(TEXT.count('${{ inputs.soak_rate }}'), 1)
        self.assertEqual(TEXT.count('${{ inputs.soak_seconds }}'), 1)
        self.assertIn('SOAK_RATE: ${{ inputs.soak_rate }}', TEXT)
        self.assertIn('SOAK_SECONDS: ${{ inputs.soak_seconds }}', TEXT)

    def test_default_dispatch_adds_no_soak_argument(self):
        self.assertEqual(TEXT.count('--soak-rate'), 1)
        line = next(line for line in TEXT.splitlines() if '--soak-rate' in line)
        self.assertIn('if [ -n "$SOAK_RATE" ]', line)
        self.assertIn('"${SOAK_ARGS[@]}"', step_block('name: Run the local load driver'))


if __name__ == '__main__':
    unittest.main()
