# Data root v1 fixture

`root/` was written by `DurableJournal::open` and `append` at `main` commit `2d60e30`, the `origin/main` merged into this branch. The events have fixed ids and the fixed time `2099-01-01T00:00:00Z`: a far-future date, so the 180-day retention rule never rejects the capture. `acknowledged_at` is the capture time, and the tests ignore it. No release tag exists yet, so this is the baseline for the first release; `WalBatch` `format_version` is 1.

Re-capture only with `SIFT_CAPTURE_WAL_V1=1 cargo test --locked -p sift --test it capture_wal_v1_fixture`, and only when the stored format changes on purpose. Never refresh it to make a failing test pass.
