#!/usr/bin/env python3
"""Offline cost model for the production acceptance. Computes from configuration; starts no resource."""
import argparse, json, math, sys

SCHEMA = 'sift.production-cost-inputs/v1'
PHASES = {'setup', 'load_30m', 'load_24h', 'faults', 'comparison'}
NUMBER_KEYS = {
    'node_count', 'node_usd_per_hour', 'driver_count', 'driver_usd_per_hour',
    'store_replicas', 'control_replicas', 'control_pvc_gib', 'pvc_usd_per_gib_month',
    'measured_store_bytes_per_item', 'rated_items_per_second', 'local_hot_retention_seconds',
    'gcs_gib', 'gcs_usd_per_gib_month', 'egress_gib', 'egress_usd_per_gib', 'uptime_check_usd',
    'usd_per_twd', 'contingency_fraction', 'approved_cap_twd',
}
KEYS = NUMBER_KEYS | {'schema', 'phases_hours'}

def require(condition, message):
    if not condition: raise ValueError(message)

def number(value, name):
    require(type(value) in (int, float) and math.isfinite(value) and value > 0, name + ' must be a finite positive number')
    return value

def estimate(inputs):
    require(type(inputs) is dict, 'inputs must be a JSON object')
    require(set(inputs) == KEYS, 'inputs must have exactly the documented keys')
    require(inputs['schema'] == SCHEMA, 'schema must be ' + SCHEMA)
    phases = inputs['phases_hours']
    require(type(phases) is dict and set(phases) == PHASES, 'phases_hours must have exactly the five phase keys')
    for name in sorted(PHASES): number(phases[name], 'phase ' + name)
    for key in sorted(NUMBER_KEYS): number(inputs[key], key)

    i = inputs
    total_hours = sum(phases.values())
    store_pvc_gib = math.ceil(i['measured_store_bytes_per_item'] * i['rated_items_per_second']
        * i['local_hot_retention_seconds'] * 2 / 1024 ** 3)
    nodes = i['node_count'] * i['node_usd_per_hour'] * total_hours
    driver = i['driver_count'] * i['driver_usd_per_hour'] * total_hours
    store = store_pvc_gib * i['store_replicas'] * i['pvc_usd_per_gib_month'] * total_hours / 720
    control = i['control_pvc_gib'] * i['control_replicas'] * i['pvc_usd_per_gib_month'] * total_hours / 720
    gcs = i['gcs_gib'] * i['gcs_usd_per_gib_month'] * total_hours / 720
    egress = i['egress_gib'] * i['egress_usd_per_gib']
    uptime = i['uptime_check_usd']
    before = nodes + driver + store + control + gcs + egress + uptime
    contingency = i['contingency_fraction'] * before

    lines = [
        {'item': 'nodes', 'usd': nodes, 'basis': f"{i['node_count']} nodes x {i['node_usd_per_hour']} USD/h x {total_hours} h"},
        {'item': 'driver', 'usd': driver, 'basis': f"{i['driver_count']} driver x {i['driver_usd_per_hour']} USD/h x {total_hours} h"},
        {'item': 'store_pvc', 'usd': store, 'basis': f"{store_pvc_gib} GiB x {i['store_replicas']} replicas x {i['pvc_usd_per_gib_month']} USD/GiB-month"},
        {'item': 'control_pvc', 'usd': control, 'basis': f"{i['control_pvc_gib']} GiB x {i['control_replicas']} replicas x {i['pvc_usd_per_gib_month']} USD/GiB-month"},
        {'item': 'gcs', 'usd': gcs, 'basis': f"{i['gcs_gib']} GiB x {i['gcs_usd_per_gib_month']} USD/GiB-month"},
        {'item': 'egress', 'usd': egress, 'basis': f"{i['egress_gib']} GiB x {i['egress_usd_per_gib']} USD/GiB"},
        {'item': 'uptime_check', 'usd': uptime, 'basis': 'fixed uptime check'},
        {'item': 'contingency', 'usd': contingency, 'basis': f"{i['contingency_fraction']} of the lines above"},
    ]
    total_usd = sum(line['usd'] for line in lines)
    total_twd = total_usd / i['usd_per_twd']
    over_cap = total_twd > i['approved_cap_twd']

    hours = 24.5 + phases['comparison']
    hourly = math.ceil((nodes + driver) / total_hours * 10000) / 10000
    fixed = max(0, total_usd - hourly * hours)
    config_values = {
        'cost_cap_usd': math.ceil(total_usd),
        'estimated_hourly_cost_usd': hourly,
        'estimated_fixed_cost_usd': fixed,
        'store_pvc_gib': store_pvc_gib,
        'control_pvc_gib': i['control_pvc_gib'],
    }
    return {
        'total_hours': total_hours,
        'store_pvc_gib': store_pvc_gib,
        'lines': lines,
        'total_usd': total_usd,
        'total_twd': total_twd,
        'approved_cap_twd': i['approved_cap_twd'],
        'over_cap': over_cap,
        'config_values': config_values,
        'needs_measurement': ['measured_store_bytes_per_item', 'node_usd_per_hour'],
    }

def render_table(result):
    rows = ['| Item | USD | Basis |', '|---|---:|---|']
    for line in result['lines']:
        rows.append(f"| {line['item']} | {line['usd']:.2f} | {line['basis']} |")
    rows.append(f"| Total | {result['total_usd']:.2f} | |")
    yes_no = 'yes' if result['over_cap'] else 'no'
    rows.append('')
    rows.append(f"Figure for approval: USD {result['total_usd']:.2f} (TWD {result['total_twd']:.0f}), "
                f"cap TWD {result['approved_cap_twd']}, over cap: {yes_no}.")
    rows.append('')
    rows.append('Needs measurement:')
    rows.extend(f'- {name}' for name in result['needs_measurement'])
    return '\n'.join(rows) + '\n'

def main():
    parser = argparse.ArgumentParser(description='Offline cost model for the production acceptance.')
    parser.add_argument('--inputs', required=True)
    parser.add_argument('--format', choices=['json', 'markdown'], default='markdown')
    args = parser.parse_args()
    try:
        with open(args.inputs, encoding='utf-8') as handle:
            result = estimate(json.load(handle))
    except ValueError as error:
        raise SystemExit('cost model refused: ' + str(error))
    if args.format == 'json':
        print(json.dumps(result, indent=2))
    else:
        print(render_table(result), end='')

if __name__ == '__main__':
    main()
