# Small Sift trial

Start with generated data. Existing application traffic is not required.
The default trial offers 100 items per second for 30 minutes.
It offers 144,000 logs, 18,000 metric points, and 18,000 trace spans.
It also creates a small native log source and a separate alert metric.

The trial uses the public HTTP API and the native log collector.
It checks each item's content and identity against saved input records.
It checks histogram buckets and complete span sets.
It resolves an error group and then checks that recurrence reopens it.
It sends real HTTPS firing and recovery notifications to a private receiver.
The receiver refuses the first delivery. The retry must keep the notification
ID and payload. The trial saves a query, a rule, and an alert mute.
It restarts Sift twice. It checks collector replay and stored data after each
restart. It reads every generated record again after the full run.
The final read scans the whole run, page by page. A private local database
tracks each expected ID. Each returned record must match its saved input.

## Local run

Build Sift from the source you want to test:

```sh
cargo build --locked -p sift --bin sift
python3 -B scripts/production/run_pilot.py \
  --binary /Users/chrischeng/faberlines/sift/target/debug/sift \
  --output-dir /private/tmp/sift-pilot-run \
  --seconds 1800 --rate 100
```

Use a new output directory. The tool creates private files under it.
It starts Sift on a free loopback port. Project access is required.
The collector has write access. The controller has project admin access.
The receiver uses a private root certificate and a server certificate.
Sift keeps normal certificate checks and the HTTPS destination allowlist.
No external notification destination is used.

The receiver uses HTTPS port 443. On macOS, use a local Docker image that has
Python 3. Pass its immutable image ID with `--receiver-image`.
Docker maps loopback port 443 to receiver port 8443.
The tool does not pull an image. The container runs with the caller's UID,
a read-only root filesystem, no Linux capabilities, and limited CPU and RAM.
It removes only the receiver container that it created.

```sh
python3 -B scripts/production/run_pilot.py \
  --binary /Users/chrischeng/faberlines/sift/target/debug/sift \
  --output-dir /private/tmp/sift-pilot-run \
  --seconds 1800 --rate 100 \
  --receiver-image sha256:REPLACE_WITH_LOCAL_PYTHON_IMAGE_ID
```

Keep `plan.json`, `inputs/`, `verified.jsonl`, `readback.sqlite`, the receiver
delivery record, and `result.json`. The result binds the exact binary hash and its embedded
source commit. Short diagnostic runs use the same checks with a smaller
`--seconds` value. Their measured duration stays in the result.
An interrupted or failed run writes `failure.json` and stops its processes.
Credential files and certificate private keys stay in the private directory.
Do not commit them or share the entire directory.

## Scope of the result

`sift.pilot-result/v1` reports `production_accepted: false`.
A passed trial proves its own offered data and operation sequence.
It does not establish 10,000 items per second, three-voter fault tolerance,
GKE operation, or GCS recovery. Those gates remain required for a rated release.

Before a GKE trial, select a cost cap. Use the existing ready node and a private
test namespace. Keep the trial bound to that node to prevent node expansion.
Use the exact tested Linux binary and record its hash.
Use private Services and bounded temporary storage for the first trial.
Check the full GKE source path separately before relying on node collectors.
Remove the trial resources after recording the result.
