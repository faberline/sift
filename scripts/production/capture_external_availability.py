#!/usr/bin/env python3
"""Collect public Cloud Monitoring observations for separate route review."""

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
from urllib.parse import urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

from prepare import candidate_module, read, require
from run_load import write

METRIC = "monitoring.googleapis.com/uptime_check/check_passed"
API = "https://monitoring.googleapis.com/v3/"
MAX_RESPONSE = 4 * 1024 * 1024


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def cloud_reader():
    token = subprocess.check_output(
        ["gcloud", "auth", "print-access-token", "--quiet"], timeout=20, text=True,
    ).strip()
    require(0 < len(token) <= 16_384 and not any(c.isspace() for c in token),
            "gcloud did not supply a usable access token")
    opener = build_opener(NoRedirect())

    def get(path, query=None):
        url = API + path
        if query is not None:
            url += "?" + urlencode(query)
        request = Request(url, headers={"Authorization": "Bearer " + token})
        with opener.open(request, timeout=15) as response:
            raw = response.read(MAX_RESPONSE + 1)
        require(0 < len(raw) <= MAX_RESPONSE, "Monitoring response exceeds the bound")
        return json.loads(raw)

    return get


def utc(value):
    require(type(value) is str and re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z", value),
            "use an RFC 3339 UTC timestamp")
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def target_host(endpoint):
    url = urlsplit(endpoint)
    host = url.hostname or ""
    require(endpoint == "https://" + host + "/readyz" and len(host) <= 253 and
            re.fullmatch(r"(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?", host),
            "endpoint must be a canonical HTTPS DNS name with /readyz and no credentials")
    return host


def collect(get, project, check_id, endpoint, start, end):
    require(re.fullmatch(r"[a-z][a-z0-9-]{4,28}[a-z0-9]", project), "invalid Monitoring project ID")
    require(re.fullmatch(r"[A-Za-z0-9_-]{1,256}", check_id), "invalid uptime check ID")
    host = target_host(endpoint)
    check_name = f"projects/{project}/uptimeCheckConfigs/{check_id}"
    config = get(check_name)
    resource = config["monitoredResource"]
    http = config["httpCheck"]
    require(config.get("name") == check_name and config.get("checkerType") == "STATIC_IP_CHECKERS"
            and not config.get("isInternal") and not config.get("disabled"),
            "uptime check is not the named active public check")
    resource_labels = {"host": host, "project_id": project}
    require(resource["type"] == "uptime_url" and
            resource["labels"] in ({"host": host}, resource_labels),
            "uptime check targets a different endpoint or project")
    require(http.get("useSsl") is True and http.get("validateSsl") is True
            and http.get("port", 443) == 443 and http.get("path") == "/readyz"
            and http.get("requestMethod", "GET") in ("GET", "METHOD_UNSPECIFIED")
            and not (http.get("headers") or http.get("authInfo") or http.get("serviceAgentAuthentication"))
            and http.get("acceptedResponseStatusCodes", []) in (
                [], [{"statusValue": 200}], [{"statusClass": "STATUS_CLASS_2XX"}],
            ),
            "uptime check must use HTTPS GET /readyz with certificate validation")

    query = {
        "filter": f'metric.type = "{METRIC}" AND metric.labels.check_id = "{check_id}" '
                  f'AND resource.type = "uptime_url" AND resource.labels.host = "{host}" '
                  f'AND resource.labels.project_id = "{project}"',
        "interval.startTime": start,
        "interval.endTime": end,
        "view": "FULL",
        "pageSize": 1000,
    }
    start_time, end_time = utc(start), utc(end)
    page_tokens, points = set(), {}
    for _ in range(32):
        page = get(f"projects/{project}/timeSeries", query)
        require(not page.get("executionErrors") and not page.get("unreachable"),
                "Monitoring returned incomplete observations")
        for series in page.get("timeSeries", []):
            metric = series["metric"]
            labels = metric["labels"]
            location = labels.get("checker_location")
            require(metric["type"] == METRIC and labels["check_id"] == check_id and
                    series["resource"] == {"type": "uptime_url", "labels": resource_labels} and
                    type(location) is str and location and not labels.get("checker_project_id"),
                    "observation identity differs from the public check")
            for point in series["points"]:
                timestamp = utc(point["interval"]["endTime"])
                value = point["value"]["boolValue"]
                key = (location, timestamp)
                require(start_time < timestamp <= end_time and type(value) is bool and key not in points,
                        "observation is outside the window, duplicated, or invalid")
                points[key] = value
        token = page.get("nextPageToken")
        if not token:
            break
        require(type(token) is str and len(token) <= 4096 and token not in page_tokens,
                "Monitoring pagination did not advance")
        page_tokens.add(token)
        query["pageToken"] = token
    else:
        raise ValueError("Monitoring observations exceeded the page limit")

    require(points and all(points.values()), "the window needs successful uptime checks and zero failures")
    return {
        "metric_type": METRIC,
        "uptime_check_sha256": hashlib.sha256(check_name.encode()).hexdigest(),
        "endpoint_sha256": hashlib.sha256(endpoint.encode()).hexdigest(),
        "successful_checks": len(points),
        "failed_checks": 0,
        "checker_locations": sorted({location for location, _ in points}),
        "window_start": start,
        "window_end": end,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate-manifest", "prepared-dir", "runtime-check", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("endpoint", "check-id", "start", "end"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    raw, candidate = read(args.candidate_manifest)
    binding = candidate_module.validate_candidate("sift", raw, candidate)
    _, preflight = read(args.prepared_dir / "preflight.json")
    _, resource = read(args.prepared_dir / "instance.json")
    _, runtime = read(args.runtime_check)
    image = "ghcr.io/faberline/sift@" + binding["root_digest"]
    identity = {
        "source_commit": binding["commit"], "image": image,
        "cluster_identity_sha256": preflight["cluster_identity_sha256"],
    }
    require(all(preflight.get(key) == runtime.get(key) == value for key, value in identity.items())
            and resource["spec"]["image"] == image and runtime.get("result") == "ready_for_load_tests"
            and runtime.get("production_accepted") is False, "candidate runtime identity differs")
    start, end = utc(args.start), utc(args.end)
    require(utc(runtime["observed_at"]) <= start < end <= datetime.now(timezone.utc) - timedelta(seconds=35)
            and end - start <= timedelta(hours=24), "use a complete window after the runtime check, up to 24 hours")
    require(not args.output.exists(), "output must be new")
    report = collect(cloud_reader(), resource["spec"]["gcpProjectId"], args.check_id,
                     args.endpoint, args.start, args.end)
    write(args.output, {"schema": "sift.external-availability-observation/v1", "result": "observed",
                        "production_accepted": False, **identity, **report})
    print("Collected external observations. Route and window coverage still require review.")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, TypeError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit("external availability refused: " + str(error))
