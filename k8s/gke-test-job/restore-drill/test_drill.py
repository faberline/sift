"""Unit checks for drill.py. Stdlib only; no cluster, no bucket, no network."""

import hashlib
import json
import os
import sys
import tempfile
import unittest


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


if __name__ == "__main__":
    unittest.main()
