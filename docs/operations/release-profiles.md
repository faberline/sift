# Release profiles (proposal)

This page proposes two release profiles for Sift and the version of the
single-node line. It changes no workflow, no script and no tag.

## Profiles

| Profile | What a release needs |
|---------|----------------------|
| `production` | The three-voter contract. A release needs the 13-gate receipt of `docs/production-v1.md`, made with `scripts/release/production_receipt.py`. |
| `single-node` | The profile of `docs/single-node-v1.md`. A release needs the single-node acceptance receipt (gates G1 to G3 of that page, run by the workflow of #237). No single-node receipt exists yet, and a release of this profile makes no claim for the three-voter topology. |

## Version and tag

The single-node line should use the version `0.3.0` and the tag `sift@0.3.0`.
`Cargo.toml` now says `0.3.0` (set by #334). The version `0.2.0` stays for the
`production` profile (#83); it is not on `main` any more. The release workflow
accepts only a plain `X.Y.Z` version, so a profile suffix cannot carry the
profile. A new profile is a minor step, so the version is `0.3.0` and not the
patch `0.2.1`, which would say that the change fixes an unreleased `0.2.0`.
The candidate workflow still refuses a version that does not match
`Cargo.toml`.

Proposal: the owner confirms before any tag

## Workflow design

- `.github/workflows/sift-release-candidate.yml`: add a `workflow_dispatch`
  input `profile`, type `choice`, options `production` and `single-node`,
  default `production`.
- `.github/workflows/sift-release.yml`: add the same `workflow_dispatch`
  input `profile`, type `choice`, options `production` and `single-node`,
  default `production`.
- Receipt script per profile: `production` calls
  `scripts/release/make-gke-release-receipt.py`,
  `scripts/release/verify-production-receipt.py` and
  `scripts/release/production_receipt.py`. `single-node` calls a new script,
  proposed as `scripts/release/make-single-node-receipt.py`, which a later
  issue writes.
- The release notes and the image labels carry `profile=<value>`.
- `scripts/release/verify-release-contract.py` pins the workflow inputs. The
  later issue changes that script and its tests in the same pull request as
  the workflows.
- Status: the input `profile` exists in both workflows (#331). The candidate
  workflow validates it. The promotion workflow stops a `single-node` run with
  `single-node receipt step not yet wired` until `make-single-node-receipt.py`
  is wired (a later child of #325).
- Status (#333): for the single-node profile the release notes carry profile=single-node and the sentence that no claim is made for the three-voter topology, and the candidate image has the label profile=single-node. The production output is unchanged.

This change is a later issue. The desk opens it after the owner confirms the
version. This page edits no workflow.

## Production profile is unchanged

The default `production` keeps every current gate, soak length and threshold.
`scripts/release/production_receipt.py` and `docs/production-v1.md` are not
edited by this change. The existing tests of the production receipt stay as
they are.

## Open decision

Confirm the version of the single-node line: 0.3.0 (recommended) or 0.2.1.
