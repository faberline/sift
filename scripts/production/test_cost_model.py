#!/usr/bin/env python3
"""Offline cost model for the 0.2.0 production acceptance. No live run, no paid resource."""
import contextlib, copy, io, json, sys, unittest
from pathlib import Path

import cost_model

INPUTS = Path(__file__).resolve().parent / 'cost-inputs.020.json'

class CostModel(unittest.TestCase):
    def setUp(self):
        self.inputs = json.loads(INPUTS.read_text())
        self.result = cost_model.estimate(self.inputs)

    def test_store_pvc_gib(self):
        self.assertEqual(cost_model.estimate(self.inputs)['store_pvc_gib'], 48280)

    def test_total_is_sum_of_lines(self):
        r = self.result
        self.assertAlmostEqual(sum(line['usd'] for line in r['lines']), r['total_usd'], delta=1e-6)
        self.assertEqual([line['item'] for line in r['lines']], [
            'nodes', 'driver', 'store_pvc', 'control_pvc', 'gcs', 'egress', 'uptime_check', 'contingency'])

    def test_config_values_cover_total(self):
        r = self.result
        v = r['config_values']
        self.assertGreaterEqual(25 * v['estimated_hourly_cost_usd'] + v['estimated_fixed_cost_usd'], r['total_usd'] - 1e-6)
        self.assertLessEqual(25 * v['estimated_hourly_cost_usd'] + v['estimated_fixed_cost_usd'], v['cost_cap_usd'])

    def test_over_cap_flag(self):
        self.assertIs(self.result['over_cap'], True)
        inputs = copy.deepcopy(self.inputs)
        inputs['approved_cap_twd'] = 10 ** 9
        self.assertIs(cost_model.estimate(inputs)['over_cap'], False)

    def test_refuses_bad_inputs(self):
        cases = []
        missing = copy.deepcopy(self.inputs); del missing['gcs_gib']; cases.append(missing)
        extra = copy.deepcopy(self.inputs); extra['x'] = 1; cases.append(extra)
        zero = copy.deepcopy(self.inputs); zero['node_count'] = 0; cases.append(zero)
        boolean = copy.deepcopy(self.inputs); boolean['node_count'] = True; cases.append(boolean)
        nan = copy.deepcopy(self.inputs); nan['node_usd_per_hour'] = float('nan'); cases.append(nan)
        phases = copy.deepcopy(self.inputs); del phases['phases_hours']['faults']; cases.append(phases)
        schema = copy.deepcopy(self.inputs); schema['schema'] = 'other'; cases.append(schema)
        for bad in cases:
            with self.subTest(bad=bad.get('schema')):
                with self.assertRaises(ValueError):
                    cost_model.estimate(bad)

    def test_markdown_has_figure_line(self):
        text = cost_model.render_table(self.result)
        self.assertIn('Figure for approval: USD', text)
        self.assertIn('Total', text)
        self.assertIn('Needs measurement', text)

    def test_main_markdown(self):
        out = io.StringIO()
        argv = sys.argv
        sys.argv = ['cost_model.py', '--inputs', str(INPUTS)]
        try:
            with contextlib.redirect_stdout(out):
                cost_model.main()
        finally:
            sys.argv = argv
        self.assertIn('Figure for approval', out.getvalue())

class SingleNodeCostModel(unittest.TestCase):
    SINGLE = Path(__file__).resolve().parent / 'cost-inputs.single-node.json'
    DOC = Path(__file__).resolve().parents[2] / 'docs/operations/single-node-acceptance-cost.md'
    PROD = INPUTS

    def test_inputs_are_accepted(self):
        cost_model.estimate(json.loads(self.SINGLE.read_text()))

    def test_one_node_and_one_replica(self):
        inputs = json.loads(self.SINGLE.read_text())
        self.assertEqual(inputs['node_count'], 1)
        self.assertEqual(inputs['store_replicas'], 1)
        self.assertEqual(inputs['control_replicas'], 1)

    def test_doc_figure_matches_the_model(self):
        inputs = json.loads(self.SINGLE.read_text())
        expected = next(line for line in cost_model.render_table(cost_model.estimate(inputs)).splitlines()
                        if line.startswith('Figure for approval:'))
        self.assertIn(expected, self.DOC.read_text().splitlines())

    def test_production_inputs_unchanged(self):
        self.assertEqual(cost_model.estimate(json.loads(self.PROD.read_text()))['store_pvc_gib'], 48280)

    def test_retention_phase_is_counted(self):
        inputs = json.loads(self.SINGLE.read_text())
        self.assertEqual(inputs['phases_hours']['retention'], 0.25)
        result = cost_model.estimate(inputs)
        self.assertEqual(result['total_hours'], 4.75)
        self.assertIs(result['over_cap'], False)
        self.assertEqual(result['config_values']['estimated_hourly_cost_usd'], 0.93)
        without = copy.deepcopy(inputs)
        del without['phases_hours']['retention']
        self.assertEqual(cost_model.estimate(without)['total_hours'], 4.5)
        zero = copy.deepcopy(inputs)
        zero['phases_hours']['retention'] = 0
        with self.assertRaises(ValueError):
            cost_model.estimate(zero)
        extra = copy.deepcopy(inputs)
        extra['phases_hours']['bogus'] = 1
        with self.assertRaises(ValueError):
            cost_model.estimate(extra)

if __name__ == '__main__':
    unittest.main()
