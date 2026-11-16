# Wire format fixtures

Each `.hex` file is lowercase hex text of the protobuf bytes that Sift accepts or emits: OTLP logs, metrics and traces (gRPC) and Prometheus Remote Write 1.0 (before snappy). The golden tests in `tests/it/wire_formats.rs` decode each file and re-encode it.

Do not regenerate these files to make a test pass. A changed byte means a changed wire format. `SIFT_CAPTURE_WIRE_FIXTURES=1 cargo test -p sift --test it wire_formats` writes them once; do not set that variable anywhere else.
