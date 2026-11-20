# Single node: where a log query spends its time

Basis: read from the code of this repository. Not measured. Not a GKE claim. No timing is stated here. It answers the two questions of issue #279; the run it comes from is https://github.com/faberline/sift/actions/runs/44511577882 (GitHub runner, 4 CPUs).

## Question 2: does the query window grow with the run?

No.

- `scripts/single_node/run_local_load.py`, `run_step`: every fifth second (`second % 5 == 4`, line 190) it runs `_queries(api, block['start_nanos'], block['end_nanos'], run)` for the block of that second.
- `scripts/production/load_data.py`, `build`: `at = base_nanos + second * 1_000_000_000` (line 24); the block has `start_nanos = at` and `end_nanos = at + 1_000_000_000` (line 91). The window is one second wide at every step.
- `main()` of `run_local_load.py` makes a new `run = uuid.uuid4().hex` (line 376) and a new `base_nanos = time.time_ns()` (line 377) for each rate step. The log filter is `attributes.run_id.value == run` (`scripts/production/load_client.py`, line 80). A step does not read the records of an earlier step, by time or by filter.
- So the table of #279 does not mix a growing time window with the rate. No fixed-window query has to be added to the load script.

## Question 1: where does a log query spend its time?

What the code does for one `POST /api/v1/query` with `signal.kind = logs`, in order:

1. `execute_query_inner` (`src/query/application/execute_query.rs`) calls `catch_up(PROJECTION_LOGGING_STORE)` (line 39). `query_logs` (`src/projection/infrastructure/shared_runtime_backend.rs`) calls `self.logging.catch_up()` again (line 50). Each query therefore applies, on the query thread, the journal events that ingest wrote since the last call. This work is shared with the write path.
2. `LoggingProjection::query` (`src/projection/infrastructure/logging_projection.rs`, line 70) takes `self.state.read()` for the whole scan. It walks `state.records.range(after_cursor + 1..)` (line 93). `records` is a `BTreeMap<u64, LogRecordV1>` keyed by cursor; no index by time exists. The first page has `after_cursor = 0`, so a window at the end of the retained records is found by a scan from the first retained record, not by a seek.
3. For each record `record_matches` (`src/projection/domain/log.rs`, line 63) parses `occurred_at` with `DateTime::parse_from_rfc3339` (line 103) after the cheaper checks. The retained records are at most `DEFAULT_RETAINED_LOG_RECORDS` = 100_000 (`src/projection/domain/log.rs`, line 11).
4. Each match is cloned (`record.clone()`, `logging_projection.rs`, line 95). A page has at most `MAX_LOG_QUERY_LIMIT` = 1_000 records (`src/shared_kernel/projection_log.rs`, line 9).
5. The `run_id` filter of the load script is not part of the projection query (`LogQuery`). `execute_query_inner` applies it after the page: for each record of the page it runs `serde_json::to_value(&record)` (`execute_query.rs`, line 101) and `evaluate_filter` (line 104).
6. In the load script, 8 of 10 offered items are logs (`scripts/production/load_data.py`, line 27: `log_count = rate * 8 // 10`). A one-second window holds 2000 log records at 2500 items/s and 4000 at 5000 items/s: 2 and 4 pages of 1000. Each page is one more request, and each request starts again at point 1.
7. The write path: `apply_idempotent` (`logging_projection.rs`, line 130) takes `self.state.write()` for each record (line 147), and when the map is over `max_records` it removes the oldest record from the text index and the map inside that write lock. A reader that holds `read()` during a scan makes each writer wait; a waiting writer can make a new reader wait.

What the code cannot tell: how long each part takes. The candidates are: the lock shared with the write path (point 7), the scan of the retained records (points 2 and 3), the filter after the page (point 5), the pages (point 6), and CPU on 4 CPUs while 12 sender threads run. Issue #295 measures them. This page makes no claim about the order of the costs.
