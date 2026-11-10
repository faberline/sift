#!/usr/bin/env python3
"""Refuse a legacy 1x1 receipt when promoting Sift 0.2.0."""
import argparse,importlib.util,json
from pathlib import Path
from production_receipt import validate_release_receipt

def main():
    p=argparse.ArgumentParser();p.add_argument('--receipt',type=Path,required=True);p.add_argument('--candidate-manifest',type=Path,required=True);a=p.parse_args()
    script=Path(__file__).with_name('make-gke-release-receipt.py')
    spec=importlib.util.spec_from_file_location('sift_candidate_receipt',script)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    raw=a.candidate_manifest.read_bytes();candidate=module.validate_candidate('sift',raw,json.loads(raw))
    receipt=json.loads(a.receipt.read_bytes())
    validate_release_receipt(receipt,candidate,'ghcr.io/faberline/sift@'+candidate['root_digest'])
    print('PASS: complete production receipt binds the exact candidate')
if __name__=='__main__':main()
