"""Check that the `## Capacity` section of the single-node page matches the result file.

Every number in the section is derived from docs/operations/single-node-capacity-result.json
with the same format as the page. A changed number in either file makes a test fail.
Stdlib only.
"""

import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULT = ROOT / 'docs' / 'operations' / 'single-node-capacity-result.json'
PAGE = ROOT / 'docs' / 'single-node-v1.md'
PRODUCTION = ROOT / 'docs' / 'production-v1.md'

LIMIT_SHARE = 0.7
PODS = (100, 300, 500)
ITEMS_PER_POD = (1, 5, 20)


def load(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def section(root=ROOT):
    lines = (Path(root) / 'docs' / 'single-node-v1.md').read_text(encoding='utf-8').splitlines()
    if '## Capacity' not in lines:
        raise AssertionError('the page has no ## Capacity heading')
    start = lines.index('## Capacity') + 1
    end = len(lines)
    for index in range(start, len(lines)):
        if lines[index].startswith('## '):
            end = index
            break
    return '\n'.join(lines[start:end])


def expected_rows(result):
    rows = []
    for step in result['steps']:
        rows.append(
            '| %d | %d | %d | %.2f | %.2f | %.2f | %.1f | %.0f |' % (
                step['rate'],
                step['accepted_items'],
                step['rejected_items'],
                step['ingest_latency_p95_seconds'],
                step['log_query_p95_seconds'],
                step['metric_query_p95_seconds'],
                step['peak_rss_bytes'] / 1e9,
                step['disk_bytes_per_accepted_item'],
            )
        )
    return rows


def limit(result):
    clean = [step['rate'] for step in result['steps']
             if step['rejected_items'] == 0 and step['error_items'] == 0]
    return max(clean)


def fit_rows(result):
    cap = limit(result)
    rows = []
    for pods in PODS:
        for each in ITEMS_PER_POD:
            total = pods * each
            fit = 'fit' if total <= LIMIT_SHARE * cap else 'not fit'
            rows.append('| %d | %d | %d | %s |' % (pods, each, total, fit))
    return rows


def machine_sentence(result):
    machine = result['machine']
    return ('measured on %s, %d CPUs, %d bytes of memory, %s, not a GKE claim'
            % (machine['platform'], machine['cpus'], machine['memory_bytes'], machine['fs_type']))


def limit_sentence(result):
    top = max(result['steps'], key=lambda step: step['rate'])
    return ('The measured limit is %d items per second: the highest step with 0 rejected '
            'and 0 error items. At %d offered the run accepted %d and rejected %d items.'
            % (limit(result), top['rate'], top['accepted_items'], top['rejected_items']))


def disk_range_sentence(result):
    values = [step['disk_bytes_per_accepted_item'] for step in result['steps']]
    return ('The data root grew by %.0f to %.0f bytes for each accepted item.'
            % (min(values), max(values)))


def worst_disk(result):
    return round(max(step['disk_bytes_per_accepted_item'] for step in result['steps']))


def pvc_line(result):
    return 'PVC bytes = items per second x 86400 x retentionDays x %d x 1.3' % worst_disk(result)


def worked_example(result):
    return ('100 items per second for 30 days needs about %.0f GB'
            % (100 * 86400 * 30 * worst_disk(result) * 1.3 / 1e9))


class CapacityDocTest(unittest.TestCase):
    def setUp(self):
        self.result = load(RESULT)
        self.text = section()

    def test_result_file_is_complete(self):
        self.assertIs(self.result['complete'], True)
        self.assertEqual(len(self.result['steps']), 5)
        self.assertEqual(self.result['claim'], 'measured on this machine, not a GKE claim')

    def test_section_exists(self):
        self.assertIn('## Capacity', PAGE.read_text(encoding='utf-8'))

    def test_machine_sentence_is_derived_from_the_file(self):
        self.assertIn(machine_sentence(self.result), self.text)

    def test_each_step_row_matches_the_file(self):
        for row in expected_rows(self.result):
            with self.subTest(row=row):
                self.assertIn(row, self.text)

    def test_limit_and_disk_range(self):
        self.assertIn(limit_sentence(self.result), self.text)
        self.assertIn(disk_range_sentence(self.result), self.text)

    def test_pvc_formula(self):
        self.assertEqual(worst_disk(self.result), 4275)
        self.assertIn(pvc_line(self.result), self.text)
        self.assertIn(worked_example(self.result), self.text)
        self.assertIn('spec.retentionDays', self.text)

    def test_fit_table_rows(self):
        for row in fit_rows(self.result):
            with self.subTest(row=row):
                self.assertIn(row, self.text)
        self.assertIn('| fit |', self.text)
        self.assertIn('| not fit |', self.text)

    def test_required_phrases(self):
        for phrase in [
            'not a GKE claim',
            'the owner has not given the real log volume',
            'Fit means ingest only',
            'three-voter topology',
            'production-v1.md',
            '#279', '#280', '#281', '#282', '#283',
        ]:
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, self.text)

    def test_production_v1_has_no_capacity_section(self):
        self.assertNotIn('## Capacity', PRODUCTION.read_text(encoding='utf-8'))

    def test_a_changed_number_fails(self):
        changed = json.loads(json.dumps(self.result))
        changed['steps'][0]['peak_rss_bytes'] = 9999999999
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'single-node-capacity-result.json'
            path.write_text(json.dumps(changed, indent=2) + '\n', encoding='utf-8')
            changed = load(path)
        self.assertNotIn(expected_rows(changed)[0], self.text)
        self.assertIn(expected_rows(self.result)[0], self.text)


if __name__ == '__main__':
    unittest.main()
