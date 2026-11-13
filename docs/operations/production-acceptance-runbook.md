# Production acceptance runbook (Sift 0.2.0)

This runbook orders the 13 gates of the 0.2.0 production contract in
`docs/production-v1.md`. It does not change that contract. Work directory:
`W=/private/tmp/sift-production`. Keep every file in `$W` private; do not commit them.

## Before you start

1. Set the cost cap first. Compute the estimate and check it against the cap:

   ```sh
   python3 -B scripts/production/cost_model.py --inputs scripts/production/cost-inputs.020.json --format json
   python3 -B scripts/production/cost_guard.py --cap-usd <cap USD> --spent-usd <estimated_fixed_cost_usd> --hourly-usd <estimated_hourly_cost_usd> --hours-left 25
   ```

   Take the two estimates from `config_values` in the first output. The guard
   exits 1 when the projected cost passes the cap.

Dispatch needs the user's approval of the figure in docs/operations/production-acceptance-cost.md.

## Run order

1. Candidate run: dispatch the workflow `.github/workflows/sift-release-candidate.yml`
   (`workflow_dispatch` inputs `version` = exact Sift semver without the `sift@` prefix,
   `commit` = exact landed main commit). Download its artifact
   `sift-release-candidate-<run id>-<run attempt>` and copy
   `candidate/final-candidate-manifest.json` (and its `.sha256`) to `$W`.

   ```sh
   gh workflow run sift-release-candidate.yml -f version=<X.Y.Z> -f commit=<sha>
   gh run download <run id> -n sift-release-candidate-<run id>-<run attempt> -D "$W/candidate-download"
   ```

2. Prepare the inputs and the deployment check (`docs/production-v1.md`, "Candidate acceptance"):

   ```sh
   python3 -B scripts/production/prepare.py \
     --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
     --config /private/tmp/sift-production/config.json \
     --cluster /private/tmp/sift-production/cluster.json \
     --samples /private/tmp/sift-production/samples.json \
     --output-dir /private/tmp/sift-production/prepared
   ```

3. Verify the live runtime (`docs/production-v1.md`):

   ```sh
   python3 -B scripts/production/verify_runtime.py \
     --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
     --prepared-dir /private/tmp/sift-production/prepared \
     --kubeconfig /private/tmp/sift-production/kubeconfig \
     --output /private/tmp/sift-production/runtime-check.json
   ```

4. Capture the external availability observation (`docs/production-v1.md`):

   ```sh
   python3 -B scripts/production/capture_external_availability.py \
     --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
     --prepared-dir /private/tmp/sift-production/prepared \
     --runtime-check /private/tmp/sift-production/runtime-check.json \
     --endpoint "$SIFT_PROBE_ENDPOINT" --check-id "$SIFT_UPTIME_CHECK_ID" \
     --start "$SIFT_PROBE_START_UTC" --end "$SIFT_PROBE_END_UTC" \
     --output /private/tmp/sift-production/evidence/external-availability-observation.json
   ```

5. Load gate `load_30m` (`docs/production-v1.md`):

   ```sh
   python3 -B scripts/production/run_load.py \
     --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
     --prepared-dir /private/tmp/sift-production/prepared \
     --samples /private/tmp/sift-production/samples.json \
     --kubeconfig /private/tmp/sift-production/kubeconfig \
     --token-file /private/tmp/sift-production/admin-token \
     --webhook-reference validation \
     --gate load_30m \
     --output-dir /private/tmp/sift-production/load-30m
   ```

6. Load gate `load_24h`: the same command with `--gate load_24h` and
   `--output-dir /private/tmp/sift-production/load-24h`. Run it only after `load_30m` passes.

7. Fault drills. Four drills are required: `node failure`, `PVC replacement`,
   `long GCS outage` and `whole-disk restore`.
   - node failure: No command exists in this repository yet; a person defines it before the first dispatch (see Open items).
     Record: `$W/evidence/node_failure.json`.
   - PVC replacement: No command exists in this repository yet; a person defines it before the first dispatch (see Open items).
     Record: `$W/evidence/pvc_restore.json`.
   - long GCS outage: No command exists in this repository yet; a person defines it before the first dispatch (see Open items).
     Record: `$W/evidence/gcs_outage.json`.
   - whole-disk restore: No command exists in this repository yet; a person defines it before the first dispatch (see Open items).
     Save one `recovery/latest.json` first, then pass that file to
     `sift restore-complete --record-file <path to recovery/latest.json>`.
     Use empty directories for the store and control data.
     Record: `$W/evidence/all_disk_restore.json`.

   The four drill gate names are the keys in `scripts/release/production_receipt.py`:
   `node_failure`, `pvc_restore`, `gcs_outage` and `all_disk_restore`.

8. Source comparison. `--output` must not exist yet (the script opens it with mode `x`).
   All four options are required. Exit 0 only when the result is `matched`.

   ```sh
   python3 -B scripts/production/compare_exports.py --before <file> --after <file> --mode <recovery|dual-write|generated> --output <file>
   ```

   Modes:
   - `generated`: `--before` is the reference export built from the saved producer records, `--after` is the Sift export.
   - `dual-write`: `--before` is the GCP export, `--after` is the Sift export of the same window.
   - `recovery`: `--before` is the Sift export before a fault, `--after` is the Sift export after it.

9. Receipt. `acceptance.json` has the schema `sift.production-acceptance/v2` and holds the
   SHA-256 of each `$W/evidence/<gate>.json` (contract: `scripts/release/production_receipt.py`).
   `--backend` must be `gcp` for `sift`.

   ```sh
   python3 scripts/release/make-gke-release-receipt.py --app sift --backend gcp --candidate-manifest "$W/final-candidate-manifest.json" --production-acceptance "$W/acceptance.json" --output "$W/sift-gke-receipt.json"
   ```

## What the workflow runs

The workflow `.github/workflows/sift-production-acceptance.yml` has two jobs:

- `guard` (every dispatch): runs `cost_model.py` and `cost_guard.py` with the cap and
  25 hours left. It fails when the projection passes the cap.
- `live` (only when `dry_run` is false): runs `setup` (`prepare.py` and `verify_runtime.py`),
  `load_30m` and `load_24h`, with a cost guard before each phase and after `load_24h`. A
  cleanup check always runs at the end.

The workflow runs only the steps above. Steps 1, 4, 7, 8 and 9 are manual.

## Naming for the cleanup check

Every node pool, disk (name, label value or description) and instance of the run must
contain `sift-prod-acc`, for example node pool `sift-prod-acc-pool`.

## Cleanup

The workflow's `cleanup check` step is last. If it fails, delete the node pool, the PVCs and
the load host by hand, then run the check again.

## Open items

A list the person must close before the first dispatch:

- (a) The `live` job's step `sign-in and private inputs (not set up)` is a fail-closed gate.
  Add GCP sign-in pinned by full commit SHA and remove the gate.
- (b) The private inputs under `$WORK`: `final-candidate-manifest.json`, `config.json`,
  `cluster.json`, `samples.json`, `admin-token` and `kubeconfig`.
- (c) The four fault drills have no command in the repository.
- (d) The `capture_external_availability.py` inputs (`SIFT_PROBE_ENDPOINT`,
  `SIFT_UPTIME_CHECK_ID`, `SIFT_PROBE_START_UTC`, `SIFT_PROBE_END_UTC`) and the reviewed
  `evidence/external-route.json` are manual.
- (e) The producer-record reference export for `--mode generated` has no builder in the repository.

No secret value is written in this runbook or in the workflow.
