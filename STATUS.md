# Sift status

Target: 0.2.0. Production acceptance is pending.
The rated target is 10,000 items per second. This rate is not yet validated.

The starting source is `3cc3a1a53cd50b2b44e47707bd2416e44dc456a0`.
Local main had 44 commits ahead of `origin/main` before this work.
No candidate image, remote CI run, release tag, or release is published.

| Phase | Local implementation | Acceptance state |
| --- | --- | --- |
| P0 | Standalone CI, pinned test environment, strict architecture, source and image receipts, production preparation | Check the exact-source candidate report below |
| P1 | General and multiline logs, durable replay, scoped CRI sources, node-local scrape, unified metric transports and histogram data | Local restart, outage, rotation, resource identity, and transport checks passed; GKE source checks remain required |
| P2 | Fixed control Raft group, bounded V2 queries, saved queries, common parser, complete joint recovery | Local control failover, restart, official Prometheus comparisons, and actual GCS-emulator restore passed |
| P3 | Error groups, alert states and durable outbox, Secret-based Webhooks, APIs, CLI, and read-only MCP | Local public API and restart flows passed; live HTTPS delivery and GKE operation flows remain required |
| P4 | Preparation, live runtime checks, 30-minute and 24-hour load drivers, complete-export comparison, strict release gates | Live load, fault, delivery, capacity, and seven-day comparison have not run |

File sources now check a saved content digest before reusing their position.
Observed rewrites record source loss and start a new source generation.
New CRI identities include file creation time and Kubernetes workload identity.
Preparation refuses application sources outside the current agent scope.
The current agents collect only the Sift instance namespace.

The earlier full test run passed 295 Rust tests. It left 24 environment tests
ignored. A separate run passed all 24 with the pinned service environment.
Those runs predate the last transport and collection fixes.
Clippy and the strict architecture check passed before those last fixes.
The final source must pass `./test.sh --candidate` before candidate readiness
is claimed. A successful gate writes [the local report](target/sift-candidate-check.json).
The report must match the current Git HEAD. A failed run removes that report.
Keep its exact source SHA and complete output with the candidate evidence.

The local load-driver check wrote and read 300 items through a real Sift
process. It ran for three seconds at 100 items per second.
It did not produce a production load receipt.
Synthetic receipt and comparison tests check refusal rules only.

The last cloud metadata read used project `axiom-502607` and Standard GKE
cluster `axiom-operator-acceptance` in `asia-east1-a`.
It found one ready node and no application Pods.
System log samples do not qualify for application storage sizing.
No paid resources or cloud test workload were created.

Remaining production work needs selected application services and namespaces,
real source samples, measured store size, and a total USD cost cap.
The selected services also determine the GCP export adapters and alert mapping.
These adapters and continuous seven-day coverage are not implemented yet.
GKE fault orchestration and full fault receipts also remain required.
The load driver and one matching export window do not replace these tests.

The same candidate must pass 30 minutes, 24 hours, all fault and recovery
drills, delivery checks, and seven-day dual-write comparison before cutover.
Keep GCP rollback settings until every gate passes.
