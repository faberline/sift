"""Fixtures for the public Cloud Monitoring evidence reader."""

from copy import deepcopy
import unittest

from capture_external_availability import METRIC, collect

PROJECT = "sift-fixture"
CHECK = "sift-fixture-check"
HOST = "sift.example.com"
ENDPOINT = "https://" + HOST + "/readyz"
START = "2026-10-01T00:00:00Z"
END = "2026-10-01T00:02:00Z"
CHECK_NAME = f"projects/{PROJECT}/uptimeCheckConfigs/{CHECK}"


class ExternalAvailabilityTests(unittest.TestCase):
    def setUp(self):
        self.config = {
            "name": CHECK_NAME,
            "checkerType": "STATIC_IP_CHECKERS",
            "monitoredResource": {"type": "uptime_url", "labels": {"host": HOST}},
            "httpCheck": {"useSsl": True, "validateSsl": True, "port": 443, "path": "/readyz"},
        }
        self.series = {
            "resource": {"type": "uptime_url", "labels": {"host": HOST, "project_id": PROJECT}},
            "metric": {"type": METRIC, "labels": {"check_id": CHECK, "checker_location": "USA"}},
            "points": [{"interval": {"endTime": "2026-10-01T00:01:00Z"}, "value": {"boolValue": True}}],
        }
        self.pages = [{"timeSeries": [self.series]}]
        self.requests = []

    def get(self, path, query=None):
        self.requests.append((path, None if query is None else dict(query)))
        if path == CHECK_NAME:
            return self.config
        self.assertEqual(path, f"projects/{PROJECT}/timeSeries")
        return self.pages.pop(0)

    def collect(self, endpoint=ENDPOINT):
        return collect(self.get, PROJECT, CHECK, endpoint, START, END)

    def test_reads_all_pages_and_returns_bound_counts(self):
        second = deepcopy(self.series)
        second["metric"]["labels"]["checker_location"] = "EUROPE"
        self.pages[0]["nextPageToken"] = "page-two"
        self.pages.append({"timeSeries": [second]})
        evidence = self.collect()
        self.assertEqual(evidence["successful_checks"], 2)
        self.assertEqual(evidence["failed_checks"], 0)
        self.assertEqual(evidence["metric_type"], METRIC)
        self.assertRegex(evidence["endpoint_sha256"], r"^[0-9a-f]{64}$")
        self.assertRegex(evidence["uptime_check_sha256"], r"^[0-9a-f]{64}$")
        self.assertIn(f'metric.labels.check_id = "{CHECK}"', self.requests[1][1]["filter"])
        self.assertEqual(self.requests[2][1]["pageToken"], "page-two")

    def test_refuses_untrusted_endpoint(self):
        with self.assertRaisesRegex(ValueError, "canonical HTTPS DNS"):
            self.collect("https://user@sift.example.com/readyz")
        self.assertFalse(self.requests)

    def test_refuses_private_check(self):
        self.config["checkerType"] = "VPC_CHECKERS"
        with self.assertRaisesRegex(ValueError, "named public check"):
            self.collect()

    def test_refuses_other_host(self):
        self.config["monitoredResource"]["labels"]["host"] = "other.example.com"
        with self.assertRaisesRegex(ValueError, "different endpoint"):
            self.collect()

    def test_refuses_a_check_which_accepts_unready_status(self):
        self.config["httpCheck"]["acceptedResponseStatusCodes"] = [{"statusValue": 503}]
        with self.assertRaisesRegex(ValueError, "HTTPS GET"):
            self.collect()

    def test_refuses_different_project_series(self):
        self.series["resource"]["labels"]["project_id"] = "other-project"
        with self.assertRaisesRegex(ValueError, "observation identity"):
            self.collect()

    def test_refuses_old_sample(self):
        self.series["points"][0]["interval"]["endTime"] = START
        with self.assertRaisesRegex(ValueError, "outside the window"):
            self.collect()

    def test_refuses_failed_sample(self):
        self.series["points"][0]["value"]["boolValue"] = False
        with self.assertRaisesRegex(ValueError, "zero failures"):
            self.collect()

    def test_refuses_missing_samples(self):
        self.pages = [{}]
        with self.assertRaisesRegex(ValueError, "successful uptime"):
            self.collect()

    def test_refuses_incomplete_response(self):
        self.pages[0]["executionErrors"] = [{"message": "partial response"}]
        with self.assertRaisesRegex(ValueError, "incomplete observations"):
            self.collect()


if __name__ == "__main__":
    unittest.main()
