"""Unit checks for smoke.py. Stdlib only; no cluster, no network."""

import contextlib
import gzip
import io
import json
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import smoke  # noqa: E402


NOW = 1_000_000_000
TRACE_ID = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
URL = "http://sift.test:7380"

GOOD = {
    "otlp_logs_partial": (200, {"partialSuccess": {"rejectedLogRecords": 1, "errorMessage": "body is required"}}),
    "otlp_metrics": (200, {}),
    "otlp_traces": (200, {}),
    "otlp_logs_gzip": (200, {}),
    "otlp_logs_protobuf": (200, {}),
    "remote_write_v1": (204, None),
    "remote_write_v2_rejected": (415, None),
    "query_logs": (200, {"partial": False, "data": {"records": [{"event_id": "smoke-log"}]}}),
    "query_metrics": (200, {"data": {"series": [{"aggregate": 42.5}]}}),
    "query_traces": (200, {"data": {"traces": [{"trace_id": TRACE_ID}]}}),
    "trace_read": (200, {"trace_id": TRACE_ID, "spans": [{"span_id": "bbbbbbbbbbbbbbbb"}],
                         "critical_path_span_ids": ["bbbbbbbbbbbbbbbb"]}),
    "correlate": (200, {"partial": False, "logs": [{"event_id": "smoke-log"}], "traces": [{"trace_id": TRACE_ID}]}),
    "log_tail": (200, {"data": {"records": [{"event_id": "smoke-log"}]}}),
}


def read_varint(data, pos):
    result = 0
    shift = 0
    while True:
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7


def fields(data):
    """Decode one protobuf message into (field number, wire type, value) tuples."""
    out = []
    pos = 0
    while pos < len(data):
        key, pos = read_varint(data, pos)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, pos = read_varint(data, pos)
        elif wire == 1:
            value = data[pos:pos + 8]
            pos += 8
        elif wire == 2:
            length, pos = read_varint(data, pos)
            value = data[pos:pos + length]
            pos += length
        else:
            raise AssertionError(f"unexpected wire type {wire}")
        out.append((number, wire, value))
    return out


def first(data, number):
    return next(value for field, _, value in fields(data) if field == number)


def strip_snappy(data):
    """Return the literal payload of a snappy raw block made by snappy_raw."""
    length, pos = read_varint(data, 0)
    tag = data[pos]
    assert tag & 3 == 0, "not a literal"
    code = tag >> 2
    if code < 60:
        size = code + 1
        pos += 1
    elif code == 61:
        size = struct.unpack("<H", data[pos + 1:pos + 3])[0] + 1
        pos += 3
    else:
        raise AssertionError(f"unexpected literal tag {tag:#x}")
    chunk = data[pos:pos + size]
    assert len(chunk) == length
    return chunk


class FakeSift:
    """Stands in for smoke.http_call. Answers by request, records every call."""

    def __init__(self, overrides=None, series=(1, 1)):
        self.responses = dict(GOOD)
        self.responses.update(overrides or {})
        self.series = list(series)
        self.calls = []

    def __call__(self, url, method, path, body=None, headers=None, timeout=10):
        head = {key.lower(): value for key, value in (headers or {}).items()}
        self.calls.append({"method": method, "path": path, "headers": head, "body": body})
        key = self.route(method, path, head, body)
        if key == "series_count":
            count = self.series.pop(0) if len(self.series) > 1 else self.series[0]
            return 200, json.dumps({"data": {"series": [{}] * count}}).encode()
        status, data = self.responses[key]
        return status, b"" if data is None else json.dumps(data).encode()

    def route(self, method, path, head, body):
        if method == "GET" and path.startswith("/api/v1/traces/"):
            return "trace_read"
        if path == "/v1/logs":
            if head.get("content-encoding") == "gzip":
                return "otlp_logs_gzip"
            if head.get("content-type") == "application/x-protobuf":
                return "otlp_logs_protobuf"
            return "otlp_logs_partial"
        if path == "/v1/metrics":
            return "otlp_metrics"
        if path == "/v1/traces":
            return "otlp_traces"
        if path == "/prometheus/api/v1/write":
            if head.get("x-prometheus-remote-write-version") == "0.1.0":
                return "remote_write_v1"
            return "remote_write_v2_rejected"
        if path == "/api/v1/query":
            signal = json.loads(body)["signal"]
            if signal["kind"] == "logs":
                return "query_logs"
            if signal["kind"] == "traces":
                return "query_traces"
            if signal.get("name") == "sift_acceptance_total":
                return "series_count"
            return "query_metrics"
        if path == "/api/v1/correlate":
            return "correlate"
        if path == "/api/v1/logs/tail":
            return "log_tail"
        raise AssertionError(f"unexpected request {method} {path}")


class PbTest(unittest.TestCase):
    def test_varint(self):
        self.assertEqual(smoke.pb_varint(0), b"\x00")
        self.assertEqual(smoke.pb_varint(300), b"\xac\x02")

    def test_string_field(self):
        self.assertEqual(smoke.pb_string(1, "ab"), b"\x0a\x02ab")

    def test_snappy_small(self):
        self.assertEqual(smoke.snappy_raw(b"abc"), b"\x03\x08abc")
        self.assertEqual(smoke.snappy_raw(b""), b"\x00")

    def test_snappy_long(self):
        data = bytes(range(256)) * 2
        out = smoke.snappy_raw(data)
        self.assertEqual(out, smoke.pb_varint(512) + b"\xf4" + struct.pack("<H", 511) + data)

    def test_logs_protobuf_fields(self):
        resource_logs = first(smoke.otlp_logs_protobuf(1000, "smoke-protobuf-0"), 1)
        scope_logs = first(resource_logs, 2)
        record = first(scope_logs, 2)
        record_fields = {number: value for number, _, value in fields(record)}
        self.assertEqual(struct.unpack("<Q", record_fields[1])[0], 1000)
        self.assertEqual(record_fields[3], b"INFO")
        self.assertEqual(first(record_fields[5], 1), b"protobuf accepted")
        key_value = record_fields[6]
        self.assertEqual(first(key_value, 1), b"sift.event_id")
        self.assertEqual(first(first(key_value, 2), 1), b"smoke-protobuf-0")

    def test_remote_write_roundtrip(self):
        write_request = strip_snappy(smoke.remote_write_v1(5000))
        series = first(write_request, 1)
        labels = []
        sample = None
        for number, _, value in fields(series):
            if number == 1:
                labels.append((first(value, 1).decode(), first(value, 2).decode()))
            elif number == 2:
                sample = value
        self.assertEqual(labels, [
            ("__name__", "sift_acceptance_total"),
            ("environment", "test"),
            ("fixture", "smoke-remote-write"),
            ("project", "local"),
        ])
        sample_fields = {number: value for number, _, value in fields(sample)}
        self.assertEqual(struct.unpack("<d", sample_fields[1])[0], 1.0)
        self.assertEqual(sample_fields[2], 5000)


class ProtocolTest(unittest.TestCase):
    def setUp(self):
        sleep = mock.patch.object(smoke.time, "sleep")
        sleep.start()
        self.addCleanup(sleep.stop)

    def run_checks(self, fake):
        with mock.patch.object(smoke, "http_call", new=fake):
            return smoke.check_protocols(URL, now_nanos=NOW)

    def test_all_pass(self):
        result = self.run_checks(FakeSift())
        self.assertEqual(result, {name: "passed" for name in [
            "otlp_logs_partial", "otlp_metrics", "otlp_traces", "otlp_logs_gzip",
            "otlp_logs_protobuf", "remote_write_v1", "remote_write_v2_rejected",
            "query_logs", "query_metrics", "query_traces", "trace_read",
            "correlate", "log_tail",
        ]})

    def test_partial_success_missing_fails(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"otlp_logs_partial": (200, {})}))
        self.assertIn("otlp_logs_partial", str(context.exception))

    def test_remote_write_wrong_status_fails(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"remote_write_v1": (200, None)}))
        self.assertIn("remote_write_v1", str(context.exception))

    def test_remote_write_v2_must_be_415(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"remote_write_v2_rejected": (204, None)}))
        self.assertIn("remote_write_v2_rejected", str(context.exception))

    def test_remote_write_v2_writes_no_data(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift(series=(1, 2)))
        self.assertIn("remote_write_v2_rejected", str(context.exception))

    def test_wrong_metric_sum_fails(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"query_metrics": (200, {"data": {"series": [{"aggregate": 42.0}]}})}))
        self.assertIn("query_metrics", str(context.exception))

    def test_missing_record_fails(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"query_logs": (200, {"partial": False, "data": {"records": []}})}))
        self.assertIn("query_logs", str(context.exception))

    def test_missing_trace_fails(self):
        with self.assertRaises(RuntimeError) as context:
            self.run_checks(FakeSift({"query_traces": (200, {"data": {"traces": []}})}))
        self.assertIn("query_traces", str(context.exception))

    def test_ingest_sends_project_header(self):
        fake = FakeSift()
        self.run_checks(fake)
        ingest = [call for call in fake.calls if call["path"].startswith(("/v1/", "/prometheus/"))]
        self.assertTrue(ingest)
        for call in ingest:
            self.assertEqual(call["headers"].get("x-sift-project"), "local", call["path"])

        gzip_call = next(call for call in ingest if call["headers"].get("content-encoding") == "gzip")
        payload = json.loads(gzip.decompress(gzip_call["body"]))
        self.assertIn("resourceLogs", payload)

        protobuf_call = next(
            call for call in ingest
            if call["headers"].get("content-type") == "application/x-protobuf" and call["path"] == "/v1/logs"
        )
        self.assertTrue(protobuf_call["body"])


class VerifyTest(unittest.TestCase):
    def test_verify_prints_protocols(self):
        pod = {
            "metadata": {"uid": "uid1"},
            "status": {"containerStatuses": [{"name": "sift", "imageID": "img"}]},
        }
        claim = {"metadata": {"uid": "pvc"}, "spec": {"volumeName": "vol"}, "status": {"phase": "Bound"}}
        event = {"message": "marker", "trace_id": "t", "span_id": "s"}

        def api(method, resource, name, uid=None):
            if method == "GET" and resource == "pods":
                return pod
            if method == "GET" and resource == "persistentvolumeclaims":
                return claim
            if method == "DELETE" and resource == "pods":
                return None
            raise AssertionError(f"unexpected {method} {resource}")

        out = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            cwd = os.getcwd()
            os.chdir(tmp)
            try:
                Path("event.jsonl").write_text(json.dumps(event) + "\n", encoding="utf-8")
                with mock.patch.object(smoke, "read_checkpoint"), \
                        mock.patch.object(smoke, "read_record", return_value="record"), \
                        mock.patch.object(smoke, "check_protocols",
                                          return_value={"otlp_logs_partial": "passed"}) as protocols, \
                        mock.patch.object(smoke, "wait_for_replacement", return_value="uid2"), \
                        contextlib.redirect_stdout(out):
                    smoke.verify(api, URL)
            finally:
                os.chdir(cwd)

        result = json.loads(out.getvalue().splitlines()[-1])
        self.assertEqual(result["result"], "PASS")
        self.assertEqual(result["protocols"], {"otlp_logs_partial": "passed"})
        protocols.assert_called_once_with(URL)


if __name__ == "__main__":
    unittest.main()
