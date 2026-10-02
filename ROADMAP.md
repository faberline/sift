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
| P4 | GKE load, fault, restore, and shadow tests | One candidate passes every production gate with linked evidence |

See [the production contract](docs/production-v1.md) for exact gates.
See [current evidence](STATUS.md) for work that has passed.

Later work includes a web UI, GCP managed-service inputs, full PromQL, and
100,000 items per second. The first release has fixed membership and shards.
