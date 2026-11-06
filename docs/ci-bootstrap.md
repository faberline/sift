# CI-only trusted verifier bootstrap

Review base: actual GitHub main
`61ba69373d8c4fc3d7303ed57fa9362c0ff451ee` (verified through the GitHub API).
This is a new isolated branch, not the P2-based CI candidate. No source/test.sh,
Cargo, Dockerfile, DDD policy, product fixture or agent configuration changes
are included. There is no test worker, full-gate runner, dependency installer
or policy-v2 contract in this bootstrap.

## Complete minimal dependency set

- `.github/workflows/sift-pr-verifier.yml`: one default-branch workflow_run job.
- `scripts/ci/verify_online.py`: platform metadata/artifact/source-tree checks;
  imports only the following local trusted helpers and Python standard library.
- `scripts/ci/pr_identity.py`: same-repository PR/open-issue checks and read-only
  GitHub API requests; it never runs the referenced PR code.
- `scripts/ci/gate_inventory.py`: fixed 16-gate inventory and nonzero-test rules.
- `scripts/ci/finalize.py`: acceptance using that fixed inventory, never a
  receipt-selected subset.
- `scripts/ci/approved-p2-driver.json`: data-only Git blob allowlist for 12
  already-reviewed future worker/driver paths. It copies no driver/product code.
- Three Python unittest files cover identity, ZIP/JSON bounds, platform run
  binding, exact inventory and approved-driver hash matching. No third-party
  Python package, Rust build, Docker or product service is needed for these units.

The workflow uses the already pinned checkout/upload-artifact actions. Runtime
permissions are only contents:read, actions:read, issues:read and
pull-requests:read. The API token is used in this separate verifier job only.

## Trust establishment and future P2 binding

1. Independently review this exact remote-main-based CI-only patch. Use the
   repository's legitimate existing human review/merge route; do not bypass
   protected checks, grant permissions, add secrets or claim that the first new
   workflow proves its own trust. It is not a trusted main commit yet.
2. After a reviewed CI-only bootstrap lands, record that main commit. GitHub
   loads workflow_run from default branch. The job checks out the immutable
   github.workflow_sha and verifies that checkout identity before importing any
   helper. No PR code or extracted artifact file is imported or executed.
3. The later P2 PR must inherit these trusted files unchanged. Its additional
   worker/driver files must match the 12 hashes in approved-p2-driver.json. Those
   hashes come from the independently reviewed P2 CI candidate patch
   `f0ab55e49e98b4606a6802c61f2e27da9275c8085b672d93d2cc64e9cbb8de5d`
   based on local P2 `3cc3a1a`. They are approved proposed file bytes, not a
   claim that the P2 product or an unpublished source commit already passed.
4. That P2 worker has no explicit API token and reports untrusted test evidence.
   It binds both source-revision variables, including the actual binary-SHA
   expectation, to the exact checkout/event head. Only the trusted verifier
   reads current API metadata and compares all protected/trusted and driver file
   blobs against its own fixed version. Changing its local copy of this
   manifest, verifier, inventory or workflow cannot change that authority.
5. Any later critical-driver change needs another independently reviewed
   allowlist/bootstrap update. Do not silently accept a PR's new hashes. P2 must
   reconcile the old P2-based verifier copies with this bootstrap's trusted
   copies instead of overwriting the bootstrap while rebasing/merging.

There is no existing upstream `sift-pr-tests` worker in this patch, so installing
this verifier does not immediately run product tests. Its first legitimate
trigger is the later source-bound worker completion. This also means the
bootstrap is not independently self-validating: human review remains the
explicit initial trust root. If the existing protected-branch process cannot
land a CI bootstrap, stop and report that exact rule; do not change it here.

## Side effects and limits

After publication, a matching completed worker starts one standard Ubuntu
verifier job capped at five minutes. It reads Sift PR/issue/workflow/artifact/tree
metadata and at most one bounded ZIP receipt without extraction. Missing,
ambiguous, changed, truncated or oversized evidence fails. It uploads only a
small head/issue/run/verifier-revision verdict for three days. Source artifacts
remain untrusted evidence; no physical execution/tamper-resistance attestation
is claimed. Review of functional tests is still required.

No GHCR, package write, check/status write, OIDC, pull_request_target, cloud
credential, GKE/GCS, release or paid larger/GPU/Apple runner is introduced.
No current push, issue/PR creation or dispatch has happened. Read-only
workflow_run verdicts are associated with default-branch runs; the controller
must manually inspect a successful verdict bound to the final PR head before
landing. No automatic required-PR-check publication or merge is claimed.

Keep a single CI-bootstrap tracking issue distinct from P2 product acceptance.
After this prerequisite, the order remains P2, production, kind scope-pending.
The canonical/module-context policy decision and full product acceptance stay
outside this bootstrap and pending their separate review/online gates.
