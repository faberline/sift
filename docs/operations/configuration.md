# Configuration reference

This page lists every `sift` command-line flag, every `SIFT_*` environment variable that the product code reads, and every field of the `Sift` custom resource. Each default is the default in the code, the help text or the CRD. A script, `scripts/verify-configuration-doc.py`, checks this page in CI. The hidden commands `acceptance-payload`, `acceptance-build-info` and `acceptance-grpc` are internal and not documented here. For the Pod label and annotations that the metric agent reads, see [metrics-scrape.md](metrics-scrape.md).

## Command line

### `sift`

Sift — one SRE product for logs, metrics, and traces

This command has no flags.

### `sift serve`

Run the unified h2c/HTTP1 operational-event service

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--role` | - | `all` | Internal Sift deployment role. All roles use the same product binary Values: all, agent, gateway, query, store, control, operator. |
| `--host` | `SIFT_HOST` | `0.0.0.0` | Address the listener binds to. |
| `--port` | `SIFT_PORT` | `7380` | HTTP listener port. |
| `--grpc-port` | `SIFT_GRPC_PORT` | - | OTLP/gRPC listener port. It defaults to 4317 for the normal HTTP port |
| `--data-dir` | `SIFT_DATA_DIR` | `/var/lib/sift` | Durable journal root. Not used with `--ephemeral`. |
| `--ephemeral` | - | - | Development-only temporary storage. Production roles refuse this flag |
| `--log-level` | `SIFT_LOG_LEVEL` | `info` | Log level, for example `info` or `debug`. |
| `--log-format` | `SIFT_LOG_FORMAT` | `json` | Values: pretty, json. |
| `--grace-secs` | `SIFT_GRACE_SECS` | `20` | Seconds that in-flight requests get to finish on shutdown. |
| `--max-body-bytes` | `SIFT_MAX_BODY_BYTES` | `1048576` | Largest accepted request body, in bytes. |
| `--otlp-endpoint` | `SIFT_OTLP_ENDPOINT` | - | OTLP endpoint given to the server's telemetry setup. |
| `--retention-days` | `SIFT_RETENTION_DAYS` | `180` | Days of events Sift keeps and accepts, 1 to 180. |

### `sift collect`

Collect structured logs, JSON, or text from a file, stdin, or Kubernetes CRI logs

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--source` | - | - | JSONL source path, or `-` for stdin |
| `--cri-root` | - | - | Kubernetes node CRI pod-log root, normally /var/log/pods |
| `--namespace` | - | - | Collect CRI logs only from this Kubernetes namespace |
| `--pod-label` | - | - | Collect CRI logs only from Pods that carry this label (`key=value`) |
| `--source-id` | - | - | Stable identity used with byte offsets to derive idempotency keys |
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL |
| `--token` | `SIFT_TOKEN` | - | Optional Sift bearer token |
| `--token-file` | `SIFT_TOKEN_FILE` | - | Rotating projected ServiceAccount token. The file is read for every request |
| `--token-audience` | `SIFT_TOKEN_AUDIENCE` | `sift.axiom.dev` | Required audience in a projected ServiceAccount token |
| `--data-dir` | `SIFT_DATA_DIR` | `/var/lib/sift` | Persistent Sift root used for agent checkpoints and rejected records |
| `--project` | - | `default` | Project name for the collected records. |
| `--environment` | - | `local` | Environment label for the collected records. |
| `--gcp-project` | `GCP_PROJECT_ID` | - | GCP project used for k8s_container monitored-resource identity |
| `--cluster` | `GKE_CLUSTER_NAME` | - | GKE cluster name for the Kubernetes resource identity. |
| `--location` | `GKE_LOCATION` | - | GKE location for the Kubernetes resource identity. |
| `--node` | `NODE_NAME` | - | Node name for the Kubernetes resource identity. |
| `--checkpoint` | - | - | Durable source offset checkpoint; defaults under DATA_DIR/agent |
| `--quarantine` | - | - | Invalid-line JSONL sink; defaults beside the checkpoint |
| `--batch-size` | - | `100` | Records per write batch. |
| `--max-line-bytes` | - | `524288` | Longest input line accepted, in bytes. |
| `--max-retries` | - | `3` | Retries of a failed write. |
| `--request-timeout-secs` | - | `10` | Timeout of each write request, in seconds. |
| `--follow` | - | - | Continue watching a regular file or refreshing CRI discovery after EOF |
| `--follow-poll-ms` | - | `250` | Poll interval in milliseconds when `--follow` is set. |
| `--format` | - | `auto` | Input format: auto, structured, json, or text Values: auto, structured, json, text. |
| `--multiline-start` | - | - | Regex that starts a new log record. Other lines join that record |
| `--metrics-port` | - | - | Expose collection lag, pending count, rejection count, and source loss |
| `--metrics-host` | - | `127.0.0.1` | Address the collector metrics listener binds to, with `--metrics-port`. |

### `sift scrape`

Collect Prometheus text metrics from Pods on this node

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--project` | - | - | Project name for the scraped metrics. |
| `--environment` | - | `production` | Environment label for the scraped metrics. |
| `--node` | `NODE_NAME` | - | Node name of this scraper. |
| `--namespace` | - | - | Restrict discovery to this namespace. Default: all namespaces |
| `--token` | `SIFT_TOKEN` | - | Bearer token for the Sift API. Not used with `--token-file`. |
| `--token-file` | `SIFT_TOKEN_FILE` | - | File with a rotating ServiceAccount token, read for each request. |
| `--state` | - | `/var/lib/sift/agent/metrics` | Directory for the scraper's state files. |
| `--interval-seconds` | - | `30` | Seconds between scrape rounds. |
| `--once` | - | - | Run one scrape round and exit. |

### `sift control`

Manage rules, saved queries, error status, and alert mutes

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL, used by every control action. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |

### `sift control list`

Usage: sift control <COLLECTION> list [OPTIONS] --project <PROJECT>

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--project` | - | - | Project to list. Sent as the `x-sift-project` header. |
| `--after` | - | - | Cursor: list only the items after this id. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |

### `sift control put`

Write a rule or saved query. The JSON includes project, request_id, expected_revision, value

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |

### `sift control delete`

Delete a rule or saved query. The JSON includes project, request_id, expected_revision

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |

### `sift control status`

Set an error group to open, resolved, or ignored

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--project` | - | - | Project of the error group. Sent as the `x-sift-project` header. |
| `--request-id` | - | - | Request id that makes a repeated write safe. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |
| `--expected-revision` | - | - | Revision the write expects. The server refuses a write on another revision. |

### `sift control mute`

Set the alert mute end as a Unix timestamp. Omit --until to unmute

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL. |
| `--project` | - | - | Project of the alert. Sent as the `x-sift-project` header. |
| `--request-id` | - | - | Request id that makes a repeated write safe. |
| `--token` | `SIFT_TOKEN` | - | Optional bearer token for the Sift API. |
| `--expected-revision` | - | - | Revision the write expects. The server refuses a write on another revision. |
| `--until` | - | - | Unix timestamp when the mute ends. Omit to unmute. |

### `sift query`

Run one versioned logs, metrics, or traces query through the Sift API

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL |
| `--token` | `SIFT_TOKEN` | - | Optional Sift bearer token |
| `--request-timeout-secs` | - | `30` | Timeout of each API request, in seconds. |

### `sift tail`

Read new logs after a cursor through the Sift API

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL |
| `--token` | `SIFT_TOKEN` | - | Optional Sift bearer token |
| `--request-timeout-secs` | - | `30` | Timeout of each API request, in seconds. |

### `sift correlate`

Correlate logs, metrics and traces by trace, span, service or attributes through the Sift API

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL |
| `--token` | `SIFT_TOKEN` | - | Optional Sift bearer token |
| `--request-timeout-secs` | - | `30` | Timeout of each API request, in seconds. |

### `sift mcp`

Serve the read-only Sift MCP tools

This command has no flags.

### `sift mcp serve`

Serve Sift tools over standard input and output

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--stdio` | - | - | Use the MCP standard-input and standard-output transport |
| `--endpoint` | `SIFT_URL` | `http://127.0.0.1:7380` | Sift service base URL used by the MCP tools |
| `--token` | `SIFT_TOKEN` | - | Optional Sift bearer token |

### `sift snapshot`

Write a consistent Sift journal snapshot to stdout or a local file

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--data-dir` | `SIFT_DATA_DIR` | `/var/lib/sift` | Journal directory to snapshot. |
| `--out` | - | - | Write the raw snapshot bytes here; omit to emit them in a JSON terminal envelope |

### `sift restore`

Restore a journal snapshot from a shared backup object URI

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--data-dir` | `SIFT_DATA_DIR` | `/var/lib/sift` | Journal directory to restore into. |
| `--source` | - | - | Source URI accepted by service-backup, for example file:///backup/sift.json |

### `sift restore-complete`

Restore data and control state from one complete recovery record

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--source` | - | - | GCS archive prefix that contains recovery/latest.json |
| `--store-dir` | - | - | Empty store PVC mount or offline store directory |
| `--control-dir` | - | - | Empty control PVC mount or offline control directory |
| `--record-file` | - | - | The same saved recovery record must be used for every replacement PVC |

### `sift backup`

Ship a live or explicitly offline journal snapshot through the shared backup contract

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--url` | `SIFT_BACKUP_URL` | - | Running Sift base URL. Fetches the protected /admin/backup snapshot |
| `--data-dir` | - | - | Legacy offline mode: open this stopped journal directly |
| `--token` | `SIFT_BACKUP_TOKEN` | - | Admin bearer token for live mode. Invalid with offline --data-dir |
| `--token-file` | `SIFT_TOKEN_FILE` | - | Rotating projected ServiceAccount token for live mode |
| `--token-audience` | `SIFT_TOKEN_AUDIENCE` | `sift.axiom.dev` | Audience required in the projected ServiceAccount token. |
| `--project` | `SIFT_PROJECT` | `*` | Project checked by Kubernetes SubjectAccessReview for live backup |
| `--dest` | - | - | Shared backup destination URI: file://, s3://, or gs:// |
| `--retention-secs` | - | - | Remove backup objects older than this many seconds after a successful write |

### `sift dockerfile`

Render source or release image Dockerfiles independently of Kubernetes

This command has no flags.

### `sift dockerfile render`

Render a source-build or release-binary Dockerfile

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--variant` | - | `release` | Values: source, release. |
| `--out` | - | - | Write to this file. Without it the output goes to stdout. |

### `sift k8s`

Render cluster CRD, operator control plane, or namespaced Sift instances

This command has no flags.

### `sift k8s crd`

Render the cluster-scoped Sift custom resource definition

This command has no flags.

### `sift k8s crd render`

Usage: sift k8s crd render [OPTIONS]

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--out` | - | - | Write to this file. Without it the output goes to stdout. |

### `sift k8s operator`

Render or run the Sift controller control plane

This command has no flags.

### `sift k8s operator render`

Render service account, RBAC, and controller deployment assets

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--namespace` | - | `sift-system` | Namespace of the operator resources. |
| `--image` | - | `ghcr.io/faberline/sift:0.2.0` | Operator container image. |
| `--out` | - | - | Write to this file. Without it the output goes to stdout. |

### `sift k8s operator run`

Controller image entrypoint. The deployed image runs this command

This command has no flags.

### `sift k8s instance`

Render one namespaced Sift custom resource

This command has no flags.

### `sift k8s instance render`

Usage: sift k8s instance render [OPTIONS]

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--profile` | - | `dev` | Values: dev, staging, prod, template. |
| `--out` | - | - | Write to this file. Without it the output goes to stdout. |

### `sift k8s collector`

Render the Sift-owned node CRI collector DaemonSet

This command has no flags.

### `sift k8s collector render`

Usage: sift k8s collector render [OPTIONS]

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--namespace` | - | `sift-system` | Namespace of the collector resources. |
| `--image` | - | `ghcr.io/faberline/sift:0.2.0` | Collector container image. |
| `--out` | - | - | Write to this file. Without it the output goes to stdout. |

### `sift connect`

Run a command through a managed Kubernetes port-forward to Sift

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--context` | - | - | kubectl context to port-forward through; omit to use the current context |
| `--namespace` | - | - | Namespace of the target Service or Sift custom resource |
| `--service` | - | - | Target Service name; defaults to `--cr` when a custom resource is named |
| `--cr` | - | - | Sift custom-resource name used to discover the target Service and token Secret |
| `--local-port` | - | - | Local port to forward to; omit to allocate an ephemeral port |
| `--remote-port` | - | `7380` | Remote Service port |
| `--secret` | - | - | Token-registry Secret name. Auto-discovered from `--cr` when omitted |
| `--token` | `SIFT_TOKEN` | - | Explicit bearer token; otherwise one is selected from the token registry |
| `--role` | - | `admin` | Minimum role required of a token selected from the registry Values: read, write, admin. |
| `--resource` | - | - | Optional resource scope used while selecting a registry token |

### `sift spec`

Print Sift's API contract or generate a typed client from it

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--format` | - | `openapi-json` | Values: openapi-json, openapi-yaml, json-schema. |

### `sift spec gen`

Generate a typed client from Sift's own OpenAPI document

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--lang` | - | - | Target language for the generated client |
| `--out` | - | - | Directory that receives the generated client files |

### `sift llm`

Print offline agent-facing operational documentation

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--topic` | - | `outline` | Documentation topic to print. |
| `--format` | - | `md` | Values: md, json. |

### `sift upgrade`

Check or install a released Sift binary

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--check` | - | - | Report the current and the latest version. Change nothing. |
| `--force` | - | - | Reinstall even when already on the selected version. |
| `--yes` | - | - | Skip the confirmation prompt. |

### `sift issue`

Search, inspect, or file Sift issues

This command has no flags.

### `sift issue search`

Usage: sift issue search [OPTIONS] [QUERY]...

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--state` | - | `open` | Values: open, closed, all. |
| `--limit` | - | `20` | Maximum number of issues to list. |

### `sift issue view`

Usage: sift issue view <NUMBER>

This command has no flags.

### `sift issue create`

Usage: sift issue create [OPTIONS] [MESSAGE]...

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--title` | - | - | Title of the issue. Short form `-t`. |
| `--dry-run` | - | - | Print the issue instead of filing it. |
| `--yes` | - | - | Skip the confirmation prompt. Short form `-y`. |

### `sift issue comment`

Usage: sift issue comment [OPTIONS] <NUMBER> [MESSAGE]...

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--dry-run` | - | - | Print the comment request without changing GitHub state. |
| `--yes` | - | - | Skip the confirmation prompt. Short form `-y`. |


## Environment variables

Variables are read at run time unless the row says otherwise. A name in backticks is the variable. `none` means the code has no default.

### Server

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_HOST` | `sift serve` (`--host`) | 0.0.0.0 | Address the listener binds to. |
| `SIFT_PORT` | `sift serve` (`--port`) | 7380 | HTTP listener port. |
| `SIFT_GRPC_PORT` | `sift serve` (`--grpc-port`) | none | OTLP/gRPC listener port. The flag doc says it defaults to 4317 for the normal HTTP port. |
| `SIFT_DATA_DIR` | `sift serve`, `snapshot`, `restore`, `collect` (`--data-dir`) | /var/lib/sift | Durable journal root. |
| `SIFT_LOG_LEVEL` | `sift serve` (`--log-level`) | info | Log level, for example `info` or `debug`. |
| `SIFT_LOG_FORMAT` | `sift serve` (`--log-format`) | json | Log format: `pretty` or `json`. |
| `SIFT_GRACE_SECS` | `sift serve` (`--grace-secs`) | 20 | Seconds that in-flight requests get to finish on shutdown. |
| `SIFT_MAX_BODY_BYTES` | `sift serve` (`--max-body-bytes`) | 1048576 | Largest accepted request body, in bytes. |
| `SIFT_OTLP_ENDPOINT` | `sift serve` (`--otlp-endpoint`) | none | OTLP endpoint given to the server's telemetry setup. |
| `SIFT_RETENTION_DAYS` | `sift serve` (`--retention-days`) | 180 | Days of events Sift keeps and accepts, 1 to 180. |
| `SIFT_PRODUCTION` | `sift serve` | none | `1`, `true` or `on` marks a production environment. Production refuses `--ephemeral`. |
| `SIFT_ENVIRONMENT` | `sift serve` | none | `prod` or `production` marks a production environment. Production refuses `--ephemeral`. |

### Ingest limits

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_MAX_COMPRESSED_BODY_BYTES` | `sift serve` | 1048576 | Largest compressed request body, in bytes. |
| `SIFT_MAX_DECODED_BODY_BYTES` | `sift serve` | 8388608 | Largest request body after decompression, in bytes. |
| `SIFT_MAX_EVENT_BYTES` | `sift serve` | 262144 | Largest single event, in bytes. |
| `SIFT_MAX_EVENTS_PER_BATCH` | `sift serve` | 1000 | Most events in one batch. |
| `SIFT_TRACE_RETAINED_BYTES` | `sift serve` | 268435456 | Most bytes of spans that the trace projection keeps in memory (the JSON size of each span). The oldest spans go first, after the span-count limit of 100,000. The newest span always stays. |
| `SIFT_MAX_CONCURRENT_INGEST_PER_PROJECT` | `sift serve`; CRD `spec.ingest.maxConcurrentRequests` | 32 | Ingest requests that run at once per project. |
| `SIFT_MAX_INFLIGHT_BODY_BYTES` | `sift serve` | 134217728 | Decoded bytes of OTLP/HTTP and remote write request bodies that run at once over all projects; a request over the limit gets 429. Must be at least `SIFT_MAX_DECODED_BODY_BYTES`. |
| `SIFT_MAX_CONCURRENT_QUERIES` | `sift serve` | 64 | Queries on the data-plane query routes that run at once; a query over the limit gets 429. |
| `SIFT_MAX_INGEST_ITEMS_PER_PROJECT_WINDOW` | `sift serve`; CRD `spec.ingest.maxItemsPerMinute` | 720000 | Items one project may send in one quota window. |
| `SIFT_INGEST_QUOTA_WINDOW_SECS` | `sift serve` | 60 | Length of the quota window, in seconds. |
| `SIFT_MAX_LOCAL_STORAGE_BYTES` | `sift serve` | 53687091200 | Local storage limit, in bytes (50 GiB). |
| `SIFT_MIN_LOCAL_FREE_BYTES` | `sift serve` | 1073741824 | Free disk space that must remain, in bytes (1 GiB). |

### Authentication

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_AUTH` | `sift serve`; CRD `spec.auth` | off | Authentication mode: `off` or `disabled` (no check), `required` (tokens), or `kubernetes`. |
| `SIFT_TOKEN_REGISTRY_FILE` | `sift serve` | none | File with the production token registry. |
| `SIFT_TOKENS` | `sift serve` | none | Inline token list for local development. Not for production. |
| `SIFT_K8S_AUDIENCE` | `sift serve` with `SIFT_AUTH=kubernetes` | sift.axiom.dev | Audience that a Kubernetes ServiceAccount token must have. |
| `SIFT_TOKEN` | `sift query`, `tail`, `correlate`, `control`, `mcp`, `collect`, `scrape` | none | Bearer token for the Sift API. `collect` and `scrape` refuse it together with `SIFT_TOKEN_FILE`. |
| `SIFT_TOKEN_FILE` | `sift collect`, `scrape`, `backup` | none | Projected ServiceAccount token file, read for each request. |
| `SIFT_TOKEN_AUDIENCE` | `sift collect`, `backup` | sift.axiom.dev | Audience required in the projected ServiceAccount token. |

### Client and CLI

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_URL` | `sift query`, `tail`, `correlate`, `control`, `mcp`, `collect`, `scrape` | http://127.0.0.1:7380 | Sift service base URL. |
| `SIFT_PROJECT` | `sift backup` (`--project`) | * | Project checked by the Kubernetes SubjectAccessReview for live backup. |
| `SIFT_API_MAX_RETRIES` | Sift query API client | 2 | Retries of a request that is safe to replay. A Rust constant, not read from the environment. |
| `SIFT_API_MAX_RETRY_DELAY` | Sift query API client | 5 seconds | Longest delay between two retries. A Rust constant, not read from the environment. |
| `SIFT_MCP_ALLOWED_HOSTS` | `sift mcp serve` | none | Comma-separated host names that the MCP endpoint accepts. Without it the code's default list applies. |
| `SIFT_MCP_ALLOWED_ORIGINS` | `sift mcp serve` | none | Comma-separated origins that the MCP endpoint accepts. Without it the code's default list applies. |

### Role endpoints

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_STORE_ENDPOINT` | `sift serve` | none | URL of the store role, which the other roles call. |
| `SIFT_STORE_GRPC_ENDPOINT` | `sift serve` | none | gRPC URL of the store role. |
| `SIFT_QUERY_ENDPOINT` | `sift serve`; control jobs | none | URL of the query role. Unset falls back to the store URL in `sift serve`. |
| `SIFT_CONTROL_ENDPOINT` | `sift serve` | none | URL of the control role, which the gateway calls. |
| `SIFT_CONTROL_PROJECTS` | `sift serve` (control jobs) | none | Comma-separated project names for the control jobs. Unset means no control jobs. |

### Archive and recovery

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_ARCHIVE_DESTINATION` | `sift serve` (store role); CRD `spec.archive.destination` | none | Archive destination URI. Unset means no archive worker. |
| `SIFT_ARCHIVE_INTERVAL_SECS` | `sift serve` (store role) | 60 | Seconds between archive runs. Must be above zero. |
| `SIFT_BOOTSTRAP_ARCHIVE_MANIFEST_URI` | `sift serve`; CRD `spec.bootstrap.archiveManifestUri` | none | Archive manifest to restore from on the first start. |
| `SIFT_RECOVERY_DESTINATION` | `sift serve` | none | Destination of the recovery archive. Setting it enables recovery. |
| `SIFT_RECOVERY_PROJECT` | `sift serve` | none | Project to recover. Required with `SIFT_RECOVERY_DESTINATION`. |
| `SIFT_BACKUP_URL` | `sift backup` (`--url`) | none | Running Sift base URL for live backup. |
| `SIFT_BACKUP_TOKEN` | `sift backup`; the backup cron job | none | Admin bearer token for live backup. |

### Peer TLS

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_PEERS` | `sift serve` (replicated roles) | none | Peer list for the replicated roles. |
| `SIFT_PEER_PORT` | `sift serve` (replicated roles) | 7381 | Port of the peer mTLS listener. |
| `SIFT_RAFT_HEADLESS` | `sift serve` (replicated roles) | sift-peer | Headless service name used to find the peers. |
| `SIFT_PEER_MTLS` | generated manifests | none | Set to `on` in the generated manifests for peer mTLS. |
| `SIFT_PEER_TLS_CERT` | `sift serve` (replicated roles) | none | Path of the peer certificate. Required for replicated roles. |
| `SIFT_PEER_TLS_KEY` | `sift serve` (replicated roles) | none | Path of the peer private key. Required for replicated roles. |
| `SIFT_PEER_TLS_CA` | `sift serve` (replicated roles) | none | Path of the CA certificate for the peers. Required for replicated roles. |

### Alerting

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_WEBHOOK_SECRET_DIRECTORY` | `sift control` jobs | /var/run/secrets/sift/webhooks | Directory with the webhook secret files. |
| `SIFT_WEBHOOK_ALLOWED_HOSTS` | `sift control` jobs; CRD `spec.alerting.allowedHosts` | none | Comma-separated hosts that alert webhooks may call. |

### Governance

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_GOVERNANCE_POLICY_FILE` | `sift serve` (ingest) | none | File with the governance policy. |

### Build metadata

| Variable | Used by | Default | Meaning |
|---|---|---|---|
| `SIFT_TARGET` | build time (`sift` tool info) | build time | Target triple of the build. Set at build time, not at run time. |
| `SIFT_GIT_SHA` | build time (`sift` tool info) | build time | Git commit of the build. Set at build time, not at run time. |
| `SIFT_BUILT_AT` | build time (`sift` tool info) | build time | Time of the build. Set at build time, not at run time. |
| `SIFT_VERSION` | release manifest bundle | none | Release version written into the release manifest bundle. Not read at run time. |

## Custom resource (Sift, sift.axiom.dev/v1alpha1)

The `Sift` resource has a `spec` (desired state) and a `status` (observed state, written by the operator). The rows below follow the CRD in `k8s/crd/sift.yaml`. A `Required` value of `yes` means the field must be set.

### Spec

| Field | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `spec` | object | no | none | Desired state of one Sift instance. |
| `spec.image` | string | yes | none | Container image of the Sift instance. |
| `spec.peerTlsSecret` | string | yes | none | Secret with the peer mTLS certificate, key and CA. |
| `spec.replicasPerShard` | integer | no | none | Replicas per shard. Fixed at 3 in this phase. |
| `spec.topology` | string | no | `ha` | Instance topology: `ha` or `singleNode`. |
| `spec.logs` | object | no | none | Log agent settings. |
| `spec.logs.namespaces` | array | no | none | Namespaces whose Pod logs the log agent collects, as `--namespace a,b`. Empty means the namespace of the instance. |
| `spec.retentionDays` | integer | no | 180 | Days of events Sift keeps and accepts, 1 to 180. The operator passes it as `--retention-days` to the store, control and all-role workloads. |
| `spec.metrics` | object | no | none | Metrics agent settings. |
| `spec.metrics.namespaces` | array | no | none | Namespaces whose Pods the metrics agent scrapes, as `--namespace a,b`. Empty means the namespace of the instance. The operator grants Pod read access only in the instance namespace; a cluster admin grants `get` and `list` on `pods` in each other listed namespace. |
| `spec.voterCount` | integer | no | none | Raft voters per shard. Fixed at 3 in this phase. |
| `spec.dataSize` | string | no | none | Deprecated. New resources use the role sizes under `spec.storage`. |
| `spec.archive` | object | no | none | Archive settings. |
| `spec.archive.destination` | string | yes | none | Archive destination URI. Sets `SIFT_ARCHIVE_DESTINATION`. |
| `spec.bootstrap` | object | no | none | Settings for the first start. |
| `spec.bootstrap.archiveManifestUri` | string | no | none | Archive manifest to restore from on the first start. Sets `SIFT_BOOTSTRAP_ARCHIVE_MANIFEST_URI`. |
| `spec.storage` | object | no | none | Volume sizes of the roles. |
| `spec.storage.storeSize` | string | no | 50Gi | Volume size of the store role. |
| `spec.storage.controlSize` | string | no | 5Gi | Volume size of the control role. |
| `spec.storage.gatewaySize` | string | no | 2Gi | Volume size of the gateway role. |
| `spec.storage.querySize` | string | no | 2Gi | Volume size of the query role. |
| `spec.storage.storageClass` | string | no | cluster default | StorageClass of the PVC of every role. Empty means the cluster default class. |
| `spec.ingest` | object | no | none | Ingest limits. |
| `spec.ingest.maxItemsPerMinute` | integer | no | 720000 | Items one project may send per minute. Sets `SIFT_MAX_INGEST_ITEMS_PER_PROJECT_WINDOW`. |
| `spec.ingest.maxConcurrentRequests` | integer | no | 32 | Ingest requests that run at once per project. Sets `SIFT_MAX_CONCURRENT_INGEST_PER_PROJECT`. |
| `spec.placement` | object | no | none | Pod placement. |
| `spec.placement.nodeSelector` | object | no | none | Node labels that the pods must match. |
| `spec.placement.tolerations` | array | no | none | Tolerations of the role Pods (store, control, query, gateway and the single Pod). Each item has `key`, `operator` (`Exists` or `Equal`), `value`, `effect` (`NoSchedule`, `PreferNoSchedule` or `NoExecute`) and `tolerationSeconds`. The log and metrics agents do not use it. |
| `spec.auth` | string | no | none | Authentication mode: `off`, `required` or `kubernetes`. Sets `SIFT_AUTH`. |
| `spec.tokensSecret` | string | no | none | Secret with the token registry for `SIFT_AUTH=required`. |
| `spec.alerting` | object | no | none | Alert webhook settings. |
| `spec.alerting.webhookSecret` | string | no | none | Secret with the webhook secret files. |
| `spec.alerting.allowedHosts` | array | no | none | Hosts that alert webhooks may call. Sets `SIFT_WEBHOOK_ALLOWED_HOSTS`. |
| `spec.gcpProjectId` | string | yes | none | GCP project of the GKE cluster. Used in the Kubernetes resource identity. |
| `spec.gkeClusterName` | string | yes | none | GKE cluster name. Used in the Kubernetes resource identity. |
| `spec.gkeLocation` | string | yes | none | GKE location. Used in the Kubernetes resource identity. |
| `spec.backup` | object | no | none | Scheduled backup settings. |
| `spec.backup.schedule` | string | yes | none | Cron schedule of the backup job. |
| `spec.backup.destination` | string | yes | none | Shared backup destination URI: `file://`, `s3://` or `gs://`. |
| `spec.backup.retentionSecs` | integer | no | none | Seconds to keep backup objects. |
| `spec.backup.adminTokenSecret` | string | no | none | Secret with the admin token of the backup job. |

### Status

| Field | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `status` | object | no | none | Observed state, written by the operator. |
| `status.phase` | string | no | none | Overall phase of the instance. |
| `status.observedGeneration` | integer | no | none | Last spec generation that the operator reconciled. |
| `status.readyReplicas` | integer | no | none | Replicas that are ready. |
| `status.desiredShardCount` | integer | no | none | Shards that the spec asks for. |
| `status.currentShardCount` | integer | no | none | Shards that exist now. |
| `status.desiredReplicasPerShard` | integer | no | none | Replicas per shard that the spec asks for. |
| `status.currentReadyReplicasPerShard` | integer | no | none | Ready replicas per shard now. |
| `status.backupPhase` | string | no | none | Phase of the last backup. |
| `status.backupMessage` | string | no | none | Message of the last backup. |
| `status.archivePhase` | string | no | none | Phase of the archive worker. |
| `status.archiveWatermark` | integer | no | none | Position up to which data is archived. |
| `status.lastArchiveManifest` | string | no | none | URI of the last archive manifest. |
| `status.restorePhase` | string | no | none | Phase of the last restore. |
| `status.restoreSourceManifest` | string | no | none | Archive manifest that the last restore read. |
| `status.backpressure` | string | no | none | Backpressure state of ingest. |
| `status.lastDataError` | string | no | none | Last data error. |
| `status.message` | string | no | none | Human-readable status message. |

