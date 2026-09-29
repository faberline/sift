# Sift docs

Project-level documentation for Sift. The product front door, install and
usage are in the root [README](../README.md); agent-facing help is `sift llm`
and [`llms.txt`](../llms.txt).

| Document | What it answers |
|----------|-----------------|
| [architecture.md](architecture.md) | How the source is cut into bounded contexts and layers, what may depend on what, and which rule breaks are allowed and why. |
| [glossary.md](glossary.md) | The terms Sift's code and docs use, and the ones that mean something different in other repos. |
| [domain/contexts.md](domain/contexts.md) | One section per context: what it owns, what each layer holds, and its recorded exceptions. |
| [adr/](adr/0001-ddd-layout.md) | Architecture decisions and the reasons behind them. |
| [operations/ha.md](operations/ha.md) | High availability: role deployment, peer TLS, backup, and recovery limits. |
| [operations/structured-stdout.md](operations/structured-stdout.md) | Structured stdout observability: what the application and the collector each own. |

The architecture contract itself is [`ddd.toml`](../ddd.toml) at the repository
root, checked by the workspace's rust-arch contract
(`scripts/meta/rust_arch_contract.py` in the workspace repo); see
[architecture.md](architecture.md#checking) for the command.
