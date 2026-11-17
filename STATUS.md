# Sift status

Target: 0.2.0. Production acceptance is pending.
The rated target is 10,000 items per second. This rate is not yet validated.

| Phase | Local implementation | Acceptance state |
| --- | --- | --- |
| P0 | Standalone CI, pinned test environment, strict architecture, source and image receipts, production preparation | Run `./test.sh --candidate` on the exact source |
| P1 | General and multiline logs, durable replay, scoped CRI sources, node-local scrape, unified metric transports and histogram data | Local restart, outage, rotation, resource identity, and transport checks passed; GKE source checks remain required |
| P2 | Fixed control Raft group, bounded V2 queries, saved queries, common parser, complete joint recovery | Local control failover, restart, official Prometheus comparisons, and actual GCS-emulator restore passed |
| P3 | Error groups, alert states and durable outbox, Secret-based Webhooks, APIs, CLI, and read-only MCP | Local public API and restart flows passed; live HTTPS delivery and GKE operation flows remain required |
| P4 | Preparation, live runtime checks, 30-minute and 24-hour load drivers, complete-export comparison, strict release gates | Live load, fault, delivery, capacity, and source comparison have not run |

File sources now check a saved content digest before reusing their position.
Observed rewrites record source loss and start a new source generation.
New CRI identities include file creation time and Kubernetes workload identity.
Preparation refuses application sources outside the current agent scope.
The current agents collect only the Sift instance namespace.

The exact-source candidate check is `./test.sh --candidate`. It needs a clean committed tree. No candidate report is kept in this repository.

The local load-driver check wrote and read 300 items through a real Sift
process. It ran for three seconds at 100 items per second.
It did not produce a production load receipt.
Synthetic receipt and comparison tests check refusal rules only.

A cloud metadata read is not production evidence. No paid resources or cloud test workload were created.

There is no existing application traffic to migrate in the current rollout.
A new installation can use a controlled test workload. It needs a complete
30-minute comparison of logs, metrics, traces, and alert events.
The comparison tool accepts generated inputs as the independent reference.
Seven-day dual-write is optional. It is not a release gate.
Use GCP source exports and keep rollback settings when migrating an existing service.
The [small trial](docs/pilot.md) has a bounded local runner.
It checks generated data, real HTTPS retry, error recurrence, and process restart.
A trial result stays separate from a production acceptance receipt.

Remaining production work needs a test namespace, generated source records,
measured store size, and a total USD cost cap. Measure samples from the actual
test workload. Fake byte estimates do not size storage.
Live source exporters, GKE fault orchestration, and full fault receipts remain
required. Application-specific GCP adapters are needed only for a migration.
The load driver and one matching export window do not replace these tests.

The same candidate must pass 30-minute and 24-hour rated load, all fault and
recovery drills, delivery checks, and complete source comparison before a
10,000-item-per-second production rating is claimed.
