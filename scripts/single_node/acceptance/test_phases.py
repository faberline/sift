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


class CostGuardTests(unittest.TestCase):
    def setUp(self):
        self.inputs = json.loads(INPUTS.read_text())

    def read_entries(self, directory):
        return json.loads((Path(directory) / phases.GUARD_FILE).read_text())

    def test_figures_before_first_phase(self):
        figures = phases.guard_figures(self.inputs, 'load_30m', 0)
        self.assertEqual(figures['hourly_usd'], 0.9301)
        self.assertEqual(figures['spent_usd'], 0)
        self.assertEqual(figures['hours_left'], 3.5)

    def test_figures_after_three_hours(self):
        figures = phases.guard_figures(self.inputs, 'faults', 10800)
        self.assertEqual(figures['hours_left'], 1.0)
        self.assertAlmostEqual(figures['spent_usd'], 2.7903, places=4)
        self.assertEqual(figures['elapsed_hours'], 3.0)

    def test_guard_lets_the_run_go_on(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(phases.run_guard('load_30m', self.inputs, '7', 0, tmp), 0)
            entry = self.read_entries(tmp)[0]
        self.assertEqual(entry['result'], 'pass')
        self.assertEqual(entry['phase'], 'load_30m')
        self.assertAlmostEqual(entry['projected_usd'], 3.2554, places=3)

    def test_guard_stops_the_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            code = phases.run_guard('load_30m', self.inputs, '3', 0, tmp)
            entry = self.read_entries(tmp)[0]
        self.assertNotEqual(code, 0)
        self.assertEqual(entry['result'], 'fail')
        self.assertIn('cost cap exceeded', entry['detail'])

    def test_spent_counts_against_the_cap(self):
        with tempfile.TemporaryDirectory() as refused, tempfile.TemporaryDirectory() as allowed:
            self.assertNotEqual(phases.run_guard('faults', self.inputs, '3.7', 10800, refused), 0)
            self.assertEqual(phases.run_guard('faults', self.inputs, '3.8', 10800, allowed), 0)

    def test_every_check_is_appended(self):
        with tempfile.TemporaryDirectory() as tmp:
            for phase, seconds in zip(phases.PHASES, (0, 1800, 9000, 10800)):
                self.assertEqual(phases.run_guard(phase, self.inputs, '7', seconds, tmp), 0)
            entries = self.read_entries(tmp)
        self.assertEqual([entry['phase'] for entry in entries], list(phases.PHASES))
        for entry in entries:
            self.assertEqual(set(entry), {'phase', 'elapsed_hours', 'spent_usd', 'hourly_usd',
                                          'hours_left', 'projected_usd', 'cap_usd', 'result',
                                          'detail'})

    def test_bad_cap_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertNotEqual(phases.run_guard('load_30m', self.inputs, 'abc', 0, tmp), 0)
            self.assertEqual(self.read_entries(tmp)[0]['result'], 'fail')

    def test_guard_command(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = ['guard', 'load_30m', '--cost-inputs', str(INPUTS), '--elapsed-seconds', '0',
                    '--evidence-dir', tmp]
            self.assertEqual(phases.main(args + ['--cap-usd', '7']), 0)
            self.assertTrue((Path(tmp) / phases.GUARD_FILE).exists())
            self.assertNotEqual(phases.main(args + ['--cap-usd', '3']), 0)


if __name__ == '__main__':
    unittest.main()
