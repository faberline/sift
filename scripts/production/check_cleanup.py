#!/usr/bin/env python3
"""Cleanup check for the production acceptance. Proves no node pool, disk or instance of the run is left. Starts no resource."""
import argparse, json, sys

def require_list(value, name):
    if type(value) is not list:
        raise ValueError(name + ' must be a list')

def leftovers_of(items, kind, label):
    found = []
    for item in items:
        if type(item) is not dict:
            raise ValueError(kind + ' items must be objects')
        texts = [str(item.get('name', ''))]
        labels = item.get('labels')
        if labels is not None:
            texts.extend(str(v) for v in labels.values())
        if item.get('description') is not None:
            texts.append(str(item['description']))
        if any(label in text for text in texts):
            found.append(f"{kind}:{item.get('name', '')}")
    return found

def check(nodepools: list, disks: list, instances: list, label: str = 'sift-prod-acc') -> dict:
    require_list(nodepools, 'nodepools')
    require_list(disks, 'disks')
    require_list(instances, 'instances')
    leftovers = []
    leftovers += leftovers_of(nodepools, 'nodepool', label)
    leftovers += leftovers_of(disks, 'disk', label)
    leftovers += leftovers_of(instances, 'instance', label)
    if not leftovers:
        return {'status': 'clean'}
    raise ValueError('leftover resources: ' + ', '.join(leftovers))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nodepools', required=True)
    parser.add_argument('--disks', required=True)
    parser.add_argument('--instances', required=True)
    parser.add_argument('--label', default='sift-prod-acc')
    args = parser.parse_args()
    try:
        with open(args.nodepools) as f: nodepools = json.load(f)
        with open(args.disks) as f: disks = json.load(f)
        with open(args.instances) as f: instances = json.load(f)
        result = check(nodepools, disks, instances, args.label)
    except ValueError as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
    print(json.dumps(result))

if __name__ == '__main__':
    main()
