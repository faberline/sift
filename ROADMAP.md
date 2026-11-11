# Sift roadmap

The next release target is 0.2.0. It serves our applications on Standard GKE.
It includes logs, metrics, traces, error groups, and alerts in one Sift binary.
The rated target is 10,000 items per second. This is a target until P4 passes.

| Phase | Work | Exit gate |
| --- | --- | --- |
| P0 | Standalone CI, candidate builds, source and image receipts, deployment entry point | Locked build, full tests, Clippy, architecture and release checks pass |
| P1 | Native log collection, durable delivery, node-local metric scraping, OTLP alignment | Raw data and identity survive rotation, outage, and restart |
| P2 | Replicated control data, V2 queries, bounded PromQL subset, saved queries | Correct queries and durable state after restart, leader change, and restore |
| P3 | Error groups, alert evaluation, durable Webhook delivery, CLI, read-only MCP | Discover, notify, resolve, recur, mute, and recover with stable notification IDs |
| P4 | GKE load, fault, restore, and source comparisons | One candidate passes every production gate with linked evidence |

See [the production contract](docs/production-v1.md) for exact gates.
See [current evidence](STATUS.md) for work that has passed.

A new installation does not need existing application traffic.
Start with a small controlled test workload. Check inputs against complete
Sift reads. Run a source comparison for at least 30 minutes before acceptance.
Seven-day dual-write is optional when an existing service moves from GCP.
The 10,000-item-per-second rating still needs both load gates and fault tests.
Use [the small trial](docs/pilot.md) to check generated data before paid GKE tests.

Later work includes a web UI, GCP managed-service inputs, full PromQL, and
100,000 items per second. The first release has fixed membership and shards.

## Core-owned operations

These parts of Sift are still Sift's own code, although core owns, or will own,
the same work. Each row moves in its own pull request after its dependency is ready.

| Row | What moves | Core crate | Needs | Sift release |
| --- | --- | --- | --- | --- |
| 1 | Restore flow, archive upload intent, commit state and GC-pending (about 934 restore lines; more in `src/archive/**`) | `service-backup` | The tag from faberline/core#147. `BackupSetV1` and `RestoreLedger` are on core `main` but in no tag yet. | After the core tag (#66) |
| 2 | Operator CRD backup spec, status patch and manual `peer_tls_secret` (about 1,393 lines in `src/operations`) | `service-k8s` | faberline/core#148 (service profile) and a core tag after faberline/core#147 | After the core tag (#66) |
| 3 | Directory swap for the dedupe index (`dedupe_replace.rs`, `dedupe_tree_copy.rs`, 304 lines) | `storage-durable` (`generation`) | The tag from faberline/core#147, if the API is not in v0.4.14 | After the core tag (#66) |
| 4 | `ephemeral_file_object_store.rs` (177 lines) | `storage-object` | None known; check the crate when the row starts | After the core tag (#66) |
| 5 | The unused `raft-core` line in `Cargo.toml` | Remove | None | 0.2.0 (#65) |

The tracking issue is #38; the children are #65 and #66.
