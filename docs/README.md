# Sift docs

Project-level documentation for Sift. The product front door, install and
usage are in the root [README](../README.md); agent-facing help is `sift llm`
and [`llms.txt`](../llms.txt).

| Document | What it answers |
|----------|-----------------|
| [production-v1.md](production-v1.md) | The 0.2.0 inputs, control state, query, load, recovery, and cutover contract. |
| [architecture.md](architecture.md) | How the source is cut into bounded contexts and layers, ports, shared service ownership and the dependency rules. |
| [public-api.md](public-api.md) | Public Rust paths and the unchanged CLI, API and persisted byte contracts. |
| [refactor-completion.md](refactor-completion.md) | Local P2 commits, checks, added cases and the existing test and document limits. |
| [glossary.md](glossary.md) | The terms Sift's code and docs use, and the ones that mean something different in other repos. |
| [domain/contexts.md](domain/contexts.md) | One section per context: what it owns, its services, ports and adapters. |
| [adr/](adr/0001-ddd-layout.md) | Architecture decisions and the reasons behind them. |
| [operations/ha.md](operations/ha.md) | High availability: role deployment, peer TLS, backup, and recovery limits. |
| [operations/public-tls.md](operations/public-tls.md) | Public API over TLS: host, certificate, Ingress and the verify check. |
| [operations/upgrade.md](operations/upgrade.md) | Version compatibility policy, rolling upgrade order and rollback rule. |
| [operations/runbook.md](operations/runbook.md) | Data-plane runbook: health, metrics, voter loss, restore, collector replay, rejected data. |
| [operations/structured-stdout.md](operations/structured-stdout.md) | Structured stdout observability: what the application and the collector each own. |
| [adr/0003-core-owned-operations.md](adr/0003-core-owned-operations.md) | Which operations Sift hands to core crates, and what Sift keeps. |

The architecture contract itself is [`ddd.toml`](../ddd.toml) at the repository
root, checked by the workspace's rust-arch contract
(`scripts/meta/rust_arch_contract.py` in the workspace repo); see
[architecture.md](architecture.md#checking) for the command.
