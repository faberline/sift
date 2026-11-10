#!/usr/bin/env python3
"""Run the Sift corpus with official Prometheus 3.5.0, without a server."""
import argparse,json,pathlib,subprocess,tempfile

def series_name(labels):
    labels=dict(labels);name=labels.pop('__name__','')
    return name+'{'+','.join(k+'='+json.dumps(v) for k,v in sorted(labels.items()))+'}'

def main():
    p=argparse.ArgumentParser();p.add_argument('--promtool',required=True);a=p.parse_args()
    version=subprocess.run([a.promtool,'--version'],capture_output=True,text=True,check=True)
    if 'version 3.5.0 ' not in version.stdout+version.stderr: raise SystemExit('official Prometheus 3.5.0 is required')
    root=pathlib.Path(__file__).resolve().parents[1]
    f=json.loads((root/'tests/fixtures/production-v1/promql-oracle.json').read_text())
    lines=['rule_files: []','evaluation_interval: 30s','tests:','  - interval: 30s','    input_series:']
    for s in f['series']:
        lines+=['      - series: '+json.dumps(series_name(s['labels'])),'        values: '+json.dumps(' '.join(str(v) for v in s['values']))]
    lines+=['    promql_expr_test:']
    for c in f['cases']:
        lines+=['      - expr: '+json.dumps(c['expression']),'        eval_time: 180s','        exp_samples:']
        if 'scalar' in c['result']:
            lines+=['          - labels: "{}"','            value: '+str(c['result']['scalar'])]
        else:
            samples=c['result']['vector']
            if not samples: lines[-1]='        exp_samples: []'
            for s in samples:lines+=['          - labels: '+json.dumps(series_name(s['labels'])),'            value: '+str(s['value'])]
    with tempfile.TemporaryDirectory(prefix='sift-promql-oracle-') as d:
        file=pathlib.Path(d)/'tests.yaml';file.write_text('\n'.join(lines)+'\n')
        subprocess.run([a.promtool,'test','rules',str(file)],check=True)
    print(f"PASS: official Prometheus 3.5.0, {len(f['cases'])} shared expressions")
if __name__=='__main__': main()
