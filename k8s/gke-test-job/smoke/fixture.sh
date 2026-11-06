#!/bin/sh
set -eu

trace_id=$(printf '%s' "${POD_UID:?}" | tr -d '-')
span_id=$(printf '%s' "$trace_id" | cut -c 1-16)
jq -cn \
  --arg timestamp "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  --arg marker "sift-gke-smoke-$POD_UID" \
  --arg trace "$trace_id" --arg span "$span_id" '
  {schema: "axiom.service.log.v1", timestamp: $timestamp, severity: "INFO",
   service: {name: "sift-gke-smoke", version: "1"},
   event: "durable_log", message: $marker, trace_id: $trace, span_id: $span,
   trace_flags: "01", attributes: {}}' >event.jsonl
