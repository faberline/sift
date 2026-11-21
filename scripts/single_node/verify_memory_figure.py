"""Check that the `## Memory` section of the single-node page matches the result file.

Every number in the section is derived from docs/operations/single-node-memory-result.json
with the same format as the page. A changed number in either file makes a test fail.
Stdlib only.
"""

import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULT = ROOT / 'docs' / 'operations' / 'single-node-memory-result.json'
PAGE = ROOT / 'docs' / 'single-node-v1.md'
PRODUCTION = ROOT / 'docs' / 'production-v1.md'

MARGIN = 1.3


def load(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def section(root=ROOT):
    lines = (Path(root) / 'docs' / 'single-node-v1.md').read_text(encoding='utf-8').splitlines()
    if '## Memory' not in lines:
        raise AssertionError('the page has no ## Memory heading')
    start = lines.index('## Memory') + 1
    end = len(lines)
    for index in range(start, len(lines)):
        if lines[index].startswith('## '):
            end = index
            break
    return '\n'.join(lines[start:end])


def gb(value):
    return '%.1f' % (value / 1e9)


def peak(result):
    return result['steps'][0]['peak_rss_bytes']


def highest_sample(result):
    return max(rss for _, rss in result['rss_samples'])


def highest_elapsed(result):
    return max(result['rss_samples'], key=lambda pair: pair[1])[0]


def machine_sentence(result):
    machine = result['machine']
    return ('measured on %s, %d CPUs, %d bytes of memory, %s, not a GKE claim'
            % (machine['platform'], machine['cpus'], machine['memory_bytes'], machine['fs_type']))


def rows(result):
    soak = result['soak_seconds']
    sample = result['sample_seconds']
    quiet = result['quiet_seconds']
    last = result['rss_samples'][-1][1]
    return [
        '| Peak RSS of the run (`peak_rss_bytes`) | %d | %s |' % (peak(result), gb(peak(result))),
        '| Highest sample during the soak (every %d s) | %d | %s |'
        % (sample, highest_sample(result), gb(highest_sample(result))),
        '| RSS at %d s, the end of ingest | %d | %s |' % (soak, last, gb(last)),
        '| RSS after %d s with no ingest (`rss_after_quiet_bytes`) | %d | %s |'
        % (quiet, result['rss_after_quiet_bytes'], gb(result['rss_after_quiet_bytes'])),
    ]


def margin_sentence(result):
    return 'peak RSS x %.1f = %s GB' % (MARGIN, '%.1f' % (peak(result) * MARGIN / 1e9))


def growth_sentence(result):
    first_elapsed, first_rss = result['rss_samples'][0]
    return ('At %d items per second the RSS grew from %d bytes at %d s to %d bytes at %d s, '
            'and did not go down in the quiet time.'
            % (result['soak_rate'], first_rss, first_elapsed,
               highest_sample(result), highest_elapsed(result)))


class MemoryFigureTest(unittest.TestCase):
    def setUp(self):
        self.result = load(RESULT)
        self.text = section()

    def test_result_file_is_complete(self):
        result = self.result
        self.assertIs(result['complete'], True)
        self.assertEqual(len(result['steps']), 1)
        self.assertEqual(result['claim'], 'measured on this machine, not a GKE claim')
        self.assertEqual(result['run_id'], 44737055221)
        self.assertEqual(len(result['rss_samples']), 20)
        for elapsed, rss in result['rss_samples']:
            self.assertIsInstance(elapsed, int)
            self.assertIsInstance(rss, int)
        self.assertEqual(result['soak_rate'], 1000)

    def test_source_run_and_machine_are_named(self):
        result = self.result
        self.assertIn(str(result['run_id']), self.text)
        self.assertIn(result['commit'], self.text)
        self.assertIn('single-node-memory-result.json', self.text)
        self.assertIn(machine_sentence(result), self.text)
        self.assertIn('not a GKE claim', self.text)

    def test_each_row_matches_the_file(self):
        for row in rows(self.result):
            with self.subTest(row=row):
                self.assertIn(row, self.text)

    def test_growth_and_margin(self):
        self.assertIn(growth_sentence(self.result), self.text)
        self.assertIn(margin_sentence(self.result), self.text)

    def test_quiet_time_did_not_go_down(self):
        result = self.result
        self.assertGreaterEqual(result['rss_after_quiet_bytes'], result['rss_samples'][-1][1])

    def test_not_measured_and_link(self):
        self.assertIn('Not measured', self.text)
        self.assertIn('GKE', self.text)
        self.assertIn('single-node-memory.md', self.text)

    def test_production_v1_is_unchanged(self):
        production = PRODUCTION.read_text(encoding='utf-8')
        self.assertNotIn('## Memory', production)
        self.assertNotIn('single-node-memory-result', production)

    def test_a_changed_number_fails(self):
        changed = json.loads(json.dumps(self.result))
        changed['steps'][0]['peak_rss_bytes'] = 9999999999
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'single-node-memory-result.json'
            path.write_text(json.dumps(changed, indent=2) + '\n', encoding='utf-8')
            changed = load(path)
        self.assertNotIn(rows(changed)[0], self.text)
        self.assertIn(rows(self.result)[0], self.text)


if __name__ == '__main__':
    unittest.main()
