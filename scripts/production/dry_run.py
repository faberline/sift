#!/usr/bin/env python3
"""Dry run for the production acceptance inputs. No cluster, network or credential is used."""
import argparse, contextlib, importlib.util, io, json, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import prepare

receipt_spec = importlib.util.spec_from_file_location(
    'production_receipt', Path(__file__).resolve().parents[1] / 'release/production_receipt.py')
receipt_module = importlib.util.module_from_spec(receipt_spec)
receipt_spec.loader.exec_module(receipt_module)

EXPECTED_GATES = ('functional', 'promql_oracle', 'control_failover', 'load_30m', 'load_24h',
                  'node_failure', 'pvc_restore', 'gcs_outage', 'all_disk_restore', 'source_fidelity',
                  'capacity', 'delivery', 'external_availability')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_manifest(resource):
    spec = resource['spec']
    require(spec['replicasPerShard'] == 3, 'replicasPerShard must be 3')
    require(spec['voterCount'] == 3, 'voterCount must be 3')
    require('@sha256:' in spec['image'], 'image must be pinned by digest')
    require(spec['storage']['storeSize'].endswith('Gi'), 'storeSize must be in Gi')
    require(spec['storage']['controlSize'].endswith('Gi'), 'controlSize must be in Gi')


def check_gate_list():
    gates = tuple(receipt_module.GATES)
    require(gates == EXPECTED_GATES, 'gate list differs from the 13 acceptance gates')
    return gates


def check_receipt_schema():
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            receipt_module.self_test()
    except (AssertionError, ValueError, KeyError) as error:
        raise ValueError('receipt schema check failed: ' + str(error))


def dry_run(candidate_raw, candidate, config, cluster, samples):
    checks = []
    resource, report = prepare.prepare(candidate_raw, candidate, config, cluster, samples)
    checks.append({'name': 'preparation', 'result': 'passed'})
    check_manifest(resource)
    checks.append({'name': 'manifest', 'result': 'passed'})
    gates = check_gate_list()
    checks.append({'name': 'gate_list', 'result': 'passed'})
    check_receipt_schema()
    checks.append({'name': 'receipt_schema', 'result': 'passed'})
    require(report['budget']['estimated_cost_usd'] <= report['budget']['cap_usd'],
            'estimated cost exceeds the cap')
    checks.append({'name': 'budget', 'result': 'passed'})
    return {'schema': 'sift.production-dry-run/v1', 'result': 'passed', 'checks': checks,
            'gates': list(gates), 'budget': report['budget'], 'production_accepted': False}


def fixture_inputs():
    candidate = prepare.candidate_module.fixture_candidate('sift', '0.2.0', 'a' * 40)
    raw = json.dumps(candidate).encode()
    config = {'project': 'project-one', 'cluster': 'acceptance', 'location': 'asia-east1-a',
              'namespace': 'app', 'name': 'sift', 'archive_uri': 'gs://project-one-backups/sift',
              'peer_tls_secret': 'sift-peer-tls', 'webhook_secret': 'sift-webhooks',
              'webhook_hosts': ['alerts.example.com'],
              'services': [{'project': 'project-one', 'namespace': 'app', 'container': 'checkout'}],
              'reference_origin': 'generated', 'comparison_hours': .5,
              'cost_cap_usd': 1000, 'estimated_hourly_cost_usd': 1, 'estimated_fixed_cost_usd': 10,
              'measured_store_bytes_per_item': 1000, 'store_pvc_gib': 50000, 'control_pvc_gib': 5}
    cluster = {'name': 'acceptance', 'location': 'asia-east1-a', 'status': 'RUNNING', 'id': 'permanent-cluster-id',
               'selfLink': 'https://container.googleapis.com/v1/projects/project-one/zones/asia-east1-a/clusters/acceptance',
               'networkConfig': {'datapathProvider': 'ADVANCED_DATAPATH', 'enableFqdnNetworkPolicy': True},
               'workloadIdentityConfig': {'workloadPool': 'project-one.svc.id.goog'}}
    samples = [{'resource': {'type': 'k8s_container', 'labels': {
        'project_id': 'project-one', 'namespace_name': 'app', 'container_name': 'checkout',
        'cluster_name': 'acceptance', 'location': 'asia-east1-a'}}, 'textPayload': 'a real input shape'}] * 100
    return raw, candidate, config, cluster, samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', action='store_true', help='build all inputs in memory')
    for name in ['candidate-manifest', 'config', 'cluster', 'samples']:
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    if args.fixture:
        inputs = fixture_inputs()
    else:
        require(all([args.candidate_manifest, args.config, args.cluster, args.samples]),
                'four input files are required without --fixture')
        candidate_raw, candidate = prepare.read(args.candidate_manifest)
        _, config = prepare.read(args.config)
        _, cluster = prepare.read(args.cluster)
        _, samples = prepare.read(args.samples, 64 * 1024 * 1024)
        inputs = (candidate_raw, candidate, config, cluster, samples)
    print(json.dumps(dry_run(*inputs), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, TypeError) as error:
        raise SystemExit('dry run refused: ' + str(error))
