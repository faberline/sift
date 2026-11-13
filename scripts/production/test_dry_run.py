#!/usr/bin/env python3
"""Offline controls for the production acceptance dry run."""
import contextlib, copy, io, json, sys, unittest
import dry_run
import prepare


class DryRun(unittest.TestCase):
    def setUp(self):
        self.inputs = dry_run.fixture_inputs()

    def test_fixture_dry_run_passes(self):
        r = dry_run.dry_run(*self.inputs)
        self.assertEqual(r['result'], 'passed')
        self.assertEqual([c['name'] for c in r['checks']],
                         ['preparation', 'manifest', 'gate_list', 'receipt_schema', 'budget'])
        for check in r['checks']:
            self.assertEqual(check['result'], 'passed')
        self.assertIs(r['production_accepted'], False)
        self.assertEqual(len(r['gates']), 13)

    def test_bad_topology_is_refused(self):
        resource, _ = dry_run.prepare.prepare(*self.inputs)
        resource['spec']['voterCount'] = 1
        with self.assertRaisesRegex(ValueError, 'voterCount'):
            dry_run.check_manifest(resource)

    def test_mutable_image_is_refused(self):
        resource, _ = dry_run.prepare.prepare(*self.inputs)
        resource['spec']['image'] = 'ghcr.io/faberline/sift:latest'
        with self.assertRaises(ValueError):
            dry_run.check_manifest(resource)

    def test_gate_list_drift_is_refused(self):
        old = dry_run.receipt_module.GATES
        dry_run.receipt_module.GATES = tuple(g for g in old if g != 'capacity')
        try:
            with self.assertRaisesRegex(ValueError, 'gate list'):
                dry_run.check_gate_list()
        finally:
            dry_run.receipt_module.GATES = old

    def test_receipt_schema_failure_is_refused(self):
        old = dry_run.receipt_module.self_test

        def broken():
            raise AssertionError('x')
        dry_run.receipt_module.self_test = broken
        try:
            with self.assertRaisesRegex(ValueError, 'receipt schema check failed'):
                dry_run.check_receipt_schema()
        finally:
            dry_run.receipt_module.self_test = old

    def test_cost_over_cap_is_refused(self):
        raw, candidate, config, cluster, samples = self.inputs
        config = copy.deepcopy(config)
        config['cost_cap_usd'] = 1
        with self.assertRaises(ValueError):
            dry_run.dry_run(raw, candidate, config, cluster, samples)

    def test_main_fixture_prints_json(self):
        old_argv = sys.argv
        sys.argv = ['dry_run.py', '--fixture']
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                dry_run.main()
        finally:
            sys.argv = old_argv
        self.assertEqual(json.loads(out.getvalue())['result'], 'passed')


if __name__ == '__main__':
    unittest.main()
