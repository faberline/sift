# Restore drill 2026-11-13

## Result

PASS. Date 2026-11-13.

Run: https://github.com/faberline/sift/actions/runs/43354079335 on `main` commit `1a18ffea` (contains PR 99). Dispatched once with `confirm=approved-300-twd` (user decision faberline/aw#339).

Jobs `resolve`, `build`, `publish` and `deploy / deploy + test on shared GKE` all passed.

Earlier run: run 43232537123 on 2026-11-12 failed with `HTTP 406` while it read the backup pod log. PR 99 (#98) fixed that. The failed run never reached the restore.

## Commands

The drill runs these two commands. See `k8s/gke-test-job/restore-drill/drill.py`, functions near lines 281 and 320.

```sh
sift backup --url <service url> --dest gs://sift-restore-drill-20261111/<run id>/backup --retention-secs 86400
sift restore --data-dir /var/lib/sift/data --source <gs uri>
```

The gs uri of this run was `gs://sift-restore-drill-20261111/6580c61b-8560-4164-9761-feccd0901eb6/backup-1794538792.json`.

## Counts and digests

- BEFORE: `count=200 sha256=c8dd1d6213a13df872887f32175a26342835a4cdd9fdb7e712320650e6220ab1`
- AFTER: `count=200 sha256=c8dd1d6213a13df872887f32175a26342835a4cdd9fdb7e712320650e6220ab1`
- The two digests are equal.
- The collector accepted 200 of 200 lines: 0 duplicates, 0 rejected, 0 lost bytes, 0 lost sources.
- The new PVC replaced the old one: old `32d2f387-7577-41ad-95fb-8b9caa297e21`, new `5c7f6be2-6cb4-4f93-88b1-5adfda51befc`.

Final JSON line of the drill:

```json
{"records": 200, "before": "c8dd1d6213a13df872887f32175a26342835a4cdd9fdb7e712320650e6220ab1", "after": "c8dd1d6213a13df872887f32175a26342835a4cdd9fdb7e712320650e6220ab1", "source": "gs://sift-restore-drill-20261111/6580c61b-8560-4164-9761-feccd0901eb6/backup-1794538792.json", "pvc_old": "32d2f387-7577-41ad-95fb-8b9caa297e21", "pvc_new": "5c7f6be2-6cb4-4f93-88b1-5adfda51befc", "result": "PASS"}
```

## Cost

This is an estimate, not a bill. The drill used about a few minutes of one node, one small pod, and a few KB in GCS. This is far below the 300 TWD limit.

- Whole run: 18.2 min (02:42:59Z to 03:01:13Z).
- GKE job: 5.7 min (02:55:31Z to 03:01:12Z).
- The rest was the image build on GitHub runners. GCP does not bill that part.

## Limits

- This drill proves the single-voter restore path only: a new PVC with the same 200 records and digest.
- It does not prove three-voter recovery. It does not use the `sift restore-complete` path.
- Spot nodes may stop during a drill.
- This was one run. A new run needs a new approval.

## Cleanup

- The GKE job deleted its objects and namespace `sift`.
- The temporary bucket `gs://sift-restore-drill-20261111` and its binding were created in faberline/aw#361 and removed in faberline/aw#362. `gcloud storage buckets describe` returns 404.
- The objects were also covered by the 7 day bucket rule.
