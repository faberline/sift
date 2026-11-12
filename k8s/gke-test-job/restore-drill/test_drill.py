"""Unit checks for drill.py. Stdlib only; no cluster, no bucket, no network."""

import hashlib
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import drill  # noqa: E402


POD_UID = "00000000-0000-4000-8000-000000000001"


class CompareTest(unittest.TestCase):
    def test_compare_equal(self):
        self.assertTrue(drill.compare(["a", "b"], ["b", "a"]))

    def test_compare_missing_record(self):
        self.assertFalse(drill.compare(["a", "b", "c"], ["a", "b"]))

    def test_compare_changed_record(self):
        self.assertFalse(drill.compare(["a", "b"], ["a", "c"]))

    def test_digest_known(self):
        self.assertEqual(drill.digest(["b", "a"]), hashlib.sha256(b"a\nb").hexdigest())


class FixtureTest(unittest.TestCase):
    def setUp(self):
        self.old_cwd = os.getcwd()
        self.old_uid = os.environ.get("POD_UID")
        self.tmp = tempfile.TemporaryDirectory()
        os.chdir(self.tmp.name)
        os.environ["POD_UID"] = POD_UID

    def tearDown(self):
        os.chdir(self.old_cwd)
        self.tmp.cleanup()
        if self.old_uid is None:
            os.environ.pop("POD_UID", None)
        else:
            os.environ["POD_UID"] = self.old_uid

    def test_fixture_lines(self):
        drill.write_fixture()
        lines = drill.SOURCE.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), 200)
        events = [json.loads(line) for line in lines]
        for event in events:
            self.assertEqual(event["service"]["name"], "sift-restore-drill")
        self.assertEqual(len({event["message"] for event in events}), 200)
        self.assertEqual(len({event["trace_id"] for event in events}), 200)


class PodSpecTest(unittest.TestCase):
    def test_pod_spec_fields(self):
        pod = drill.pod_spec(
            "sift-restore-drill-backup",
            "sift-client",
            "sift-restore-drill",
            {"app.kubernetes.io/name": "sift-restore-drill-probe"},
            [{
                "name": "sift",
                "image": "example.invalid/sift@sha256:" + "0" * 64,
                "args": ["backup"],
                "resources": {"requests": {"cpu": "100m", "memory": "128Mi"}},
            }],
        )
        spec = pod["spec"]
        self.assertIs(spec["automountServiceAccountToken"], False)
        self.assertEqual(spec["imagePullSecrets"], [{"name": "ghcr-pull"}])
        self.assertEqual(spec["nodeSelector"], {"faberline.dev/pool": "sift-client"})
        self.assertEqual(spec["tolerations"], [{
            "key": "faberline.dev/pool",
            "operator": "Equal",
            "value": "sift-client",
            "effect": "NoSchedule",
        }])
        self.assertEqual(spec["serviceAccountName"], "sift-restore-drill")
        self.assertEqual(pod["metadata"]["labels"]["app.kubernetes.io/part-of"], "sift-restore-drill")


class FakeResponse:
    def __init__(self, status, body):
        self.status = status
        self._body = body

    def read(self):
        return self._body.encode()


class FakeConnection:
    instances = []
    status = 200
    body = ""

    def __init__(self, *args, **kwargs):
        self.headers = None
        FakeConnection.instances.append(self)

    def request(self, method, path, body=None, headers=None):
        self.method, self.path, self.headers = method, path, headers

    def getresponse(self):
        return FakeResponse(FakeConnection.status, FakeConnection.body)

    def close(self):
        pass


class KubeApiTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        Path(self.tmp.name, "token").write_text("abc")
        self.patches = [
            mock.patch.object(drill, "SERVICE_ACCOUNT", Path(self.tmp.name)),
            mock.patch.object(drill.ssl, "create_default_context"),
            mock.patch.object(drill.http.client, "HTTPSConnection", FakeConnection),
        ]
        for patch in self.patches:
            patch.start()
        FakeConnection.instances = []

    def tearDown(self):
        for patch in self.patches:
            patch.stop()
        self.tmp.cleanup()

    def respond(self, status, body):
        FakeConnection.status = status
        FakeConnection.body = body

    def test_log_read_accepts_text_plain(self):
        self.respond(200, '{"object": {"key": "k"}}\n')
        result = drill.kube_api("GET", "/api/v1/namespaces/sift/pods/p/log")
        self.assertEqual(result, '{"object": {"key": "k"}}\n')
        self.assertIsInstance(result, str)
        self.assertEqual(FakeConnection.instances[0].headers["Accept"], "text/plain, */*")

    def test_json_read_accepts_json(self):
        self.respond(200, '{"a": 1}')
        result = drill.kube_api("GET", "/api/v1/namespaces/sift/pods/p")
        self.assertEqual(result, {"a": 1})
        self.assertEqual(FakeConnection.instances[0].headers["Accept"], "application/json")

    def test_error_carries_body(self):
        self.respond(406, "not acceptable: no matching media type")
        with self.assertRaises(RuntimeError) as raised:
            drill.kube_api("GET", "/api/v1/namespaces/sift/pods/p/log")
        message = str(raised.exception)
        self.assertIn("HTTP 406", message)
        self.assertIn("not acceptable: no matching media type", message)

    def test_404_get_returns_none(self):
        self.respond(404, "")
        self.assertIsNone(drill.kube_api("GET", "/api/v1/namespaces/sift/pods/p"))

    def test_error_body_is_truncated(self):
        self.respond(500, "x" * 2000)
        with self.assertRaises(RuntimeError) as raised:
            drill.kube_api("GET", "/api/v1/namespaces/sift/pods/p/log")
        message = str(raised.exception)
        self.assertLess(len(message), 700)
        self.assertIn("x" * 500, message)
        self.assertNotIn("x" * 501, message)


if __name__ == "__main__":
    unittest.main()
