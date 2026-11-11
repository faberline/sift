# WAL v1 fixture

`wal/{logs,metrics,traces}/events.framed` were written by `SignalWal::append` at `main` commit `cb80ef0` with fixed event ids and times. No release tag exists yet, so this is the baseline for the first release; `WalBatch` `format_version` is 1.

Re-capture only with `SIFT_CAPTURE_WAL_V1=1` and only when the format version changes on purpose. Never refresh it to make a failing test pass.
