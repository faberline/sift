"""Tests for the phases of the single-node acceptance. Standard library only."""
import json, re, sys, tempfile, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'production'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import phases  # noqa: E402

INPUTS = Path(__file__).resolve().parents[2] / 'production' / 'cost-inputs.single-node.json'
WORKFLOW = Path(__file__).resolve().parents[3] / '.github' / 'workflows' / 'sift-single-node-acceptance.yml'
TIMESTAMP = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$')


class PhaseTests(unittest.TestCase):
    def test_empty_phase_fails(self):
        result = phases.phase_result('load_30m', 'a', 'b', 0, 0, 1)
        self.assertEqual(result['result'], 'fail')

    def test_phase_with_items_passes(self):
        result = phases.phase_result('faults', 'a', 'b', 10, 10, 1)
        self.assertEqual(result['result'], 'pass')

    def test_partial_accept_fails(self):
        result = phases.phase_result('faults', 'a', 'b', 10, 9, 1)
        self.assertEqual(result['result'], 'fail')

    def test_evidence_has_every_key(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = phases.phase_result('comparison', phases.utc_now(), phases.utc_now(), 5, 5, 1)
            path = phases.write_evidence(Path(tmp), result)
            self.assertEqual(path.name, 'comparison.json')
            data = json.loads(path.read_text())
        self.assertEqual(set(data), {'phase', 'start', 'end', 'selected', 'accepted',
                                     'node_count', 'result', 'detail'})
        self.assertRegex(data['start'], TIMESTAMP)
        self.assertRegex(data['end'], TIMESTAMP)

    def test_timeout_is_hours_plus_20_percent(self):
        self.assertEqual(phases.phase_timeout_seconds(0.5), 2160)
        self.assertEqual(phases.phase_timeout_seconds(2.0), 8640)

    def test_load_seconds(self):
        self.assertEqual(phases.load_seconds(0.5), 1680)
        self.assertEqual(phases.load_seconds(2.0), 7080)

    def test_total_fits_ttl(self):
        self.assertIsNone(phases.check_total(phases.read_hours(INPUTS), 300))
        with self.assertRaises(ValueError):
            phases.check_total(phases.read_hours(INPUTS), 270)

    def test_load_rate(self):
        self.assertEqual(phases.load_rate(json.loads(INPUTS.read_text())), 50)
        for bad in ({'rated_items_per_second': 1001},
                    {'rated_items_per_second': 55},
                    {'rated_items_per_second': 'x'}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                phases.load_rate(bad)

    def test_phases_match_cost_inputs(self):
        self.assertEqual(set(phases.PHASES) | {'setup'}, set(phases.read_hours(INPUTS)))

    def test_ttl_matches_workflow(self):
        self.assertIn(f'ttl-minutes: {phases.TTL_MINUTES}', WORKFLOW.read_text())


if __name__ == '__main__':
    unittest.main()
