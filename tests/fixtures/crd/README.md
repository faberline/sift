# CRD schema golden file

`sift-crd.json` is the JSON form of the `Sift` CRD in `k8s/crd/sift.yaml`. A change of the CRD must change this file on purpose, in the same pull request.

It is created once by the capture run: `SIFT_CAPTURE_CRD_SCHEMA=1 cargo test -p sift --test it crd_schema`. Do not run the capture to make a failing test pass.
