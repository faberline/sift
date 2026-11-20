# Sift docs

Project-level documentation for Sift. The product front door, install and
usage are in the root [README](../README.md); agent-facing help is `sift llm`
and [`llms.txt`](../llms.txt).

| Document | What it answers |
|----------|-----------------|
| [production-v1.md](production-v1.md) | The 0.2.0 inputs, control state, query, load, recovery, and cutover contract. |
| [single-node-v1.md](single-node-v1.md) | The single-node profile contract: promises, limits and acceptance gates. |
| [single-node-log-query.md](single-node-log-query.md) | Single node: where a log query spends its time under ingest load, read from the code. |
| [operations/release-profiles.md](operations/release-profiles.md) | Proposal: the production and single-node release profiles, the version and the workflow design. |
| [architecture.md](architecture.md) | How the source is cut into bounded contexts and layers, ports, shared service ownership and the dependency rules. |
| [public-api.md](public-api.md) | Public Rust paths and the unchanged CLI, API and persisted byte contracts. |
| [glossary.md](glossary.md) | The terms Sift's code and docs use, and the ones that mean something different in other repos. |
| [domain/contexts.md](domain/contexts.md) | One section per context: what it owns, its services, ports and adapters. |
| [adr/](adr/0001-ddd-layout.md) | Architecture decisions and the reasons behind them. |
| [operations/ha.md](operations/ha.md) | High availability: role deployment, peer TLS, backup, and recovery limits. |
| [operations/public-tls.md](operations/public-tls.md) | Public API over TLS: host, certificate, Ingress and the verify check. |
| [operations/upgrade.md](operations/upgrade.md) | Version compatibility policy, rolling upgrade order and rollback rule. |
| [operations/runbook.md](operations/runbook.md) | Data-plane runbook: health, metrics, voter loss, restore, collector replay, rejected data. |
| [operations/install.md](operations/install.md) | The four install paths (install.sh, container image, Compose, Kubernetes) and how each command is tested. |
| [operations/configuration.md](operations/configuration.md) | Every CLI flag, environment variable and CRD field with default and meaning. |
| [operations/metrics-scrape.md](operations/metrics-scrape.md) | Pod label and annotations that the metric agent scrapes, and how they map from a GMP PodMonitoring. |
| [operations/log-and-metrics-labels.md](operations/log-and-metrics-labels.md) | The two Pod labels, `sift.axiom.dev/logs` and `sift.axiom.dev/scrape`, for one Deployment: what each one turns on, and the namespace rule. |
| [operations/limits.md](operations/limits.md) | Every limit with its value and where the code enforces it, and the known limits of this release. |
| [operations/structured-stdout.md](operations/structured-stdout.md) | Structured stdout observability: what the application and the collector each own. |
| [security/threat-model.md](security/threat-model.md) | Trust boundaries of Sift, the control at each one and the test or CI job that proves it. |
| [operations/performance.md](operations/performance.md) | Metric workload numbers, machine class, regression threshold and the CI gate. |
| [adr/0003-core-owned-operations.md](adr/0003-core-owned-operations.md) | Which operations Sift hands to core crates, and what Sift keeps. |

The architecture contract itself is [`ddd.toml`](../ddd.toml) at the repository
root. Run the check with `scripts/check-architecture.sh`; see
[architecture.md](architecture.md#checking) for the command.
