#!/bin/sh
set -eu

kubectl() {
  command kubectl --request-timeout=5s "$@"
}

marker=$(jq -er .message event.jsonl)
trace_id=$(jq -er .trace_id event.jsonl)
span_id=$(jq -er .span_id event.jsonl)
jq -e --arg source "$marker" --argjson bytes "$(wc -c <event.jsonl)" '
  .schema == "collector.checkpoint.v1" and .source_id == $source
  and .line == 1 and .offset == $bytes
  and .accepted == 1 and .duplicates == 0 and .rejected == 0
' checkpoint.json >/dev/null
test ! -s rejected.jsonl

query=$(jq -cn --arg trace "$trace_id" '
  {version: 1, project: "local", environment: "test", mode: "sync", limit: 2,
   signal: {kind: "logs", filter: {op: "eq", field: "trace_id", value: $trace}}}')

read_record() {
  for _ in $(seq 1 30); do
    if response=$(curl --silent --fail --max-time 3 \
      --header 'Content-Type: application/json' --data "$query" \
      "${SIFT_URL:?}/api/v1/query") &&
      record=$(printf '%s\n' "$response" |
        jq -ceS --arg marker "$marker" --arg trace "$trace_id" --arg span "$span_id" '
          .data.records | select(length == 1) | .[0] |
          select(.body_text == $marker and .trace_id == $trace and .span_id == $span
            and .resource["service.name"] == "sift-gke-smoke"
            and .attributes["event.name"].value == "durable_log")'); then
      printf '%s\n' "$record"
      return 0
    fi
    sleep 2
  done
  echo 'Sift query did not return the exact fixture record' >&2
  return 1
}

pod=sift-smoke-0
claim=data-sift-smoke-0
original_pod=$(kubectl get pod "$pod" -o json)
original_uid=$(printf '%s\n' "$original_pod" | jq -er .metadata.uid)
image_id=$(printf '%s\n' "$original_pod" |
  jq -er '.status.containerStatuses[] | select(.name == "sift") | .imageID | select(length > 0)')
original_claim=$(kubectl get pvc "$claim" -o json)
claim_uid=$(printf '%s\n' "$original_claim" | jq -er .metadata.uid)
volume=$(printf '%s\n' "$original_claim" |
  jq -er 'select(.status.phase == "Bound") | .spec.volumeName | select(length > 0)')
before=$(read_record)

kubectl delete pod "$pod" --wait=false
new_uid=
for _ in $(seq 1 90); do
  if candidate=$(kubectl get pod "$pod" --ignore-not-found -o json) &&
    new_uid=$(printf '%s\n' "$candidate" | jq -er --arg old "$original_uid" --arg image "$image_id" '
      select(.metadata.uid != $old and .metadata.deletionTimestamp == null
        and any(.status.conditions[]?; .type == "Ready" and .status == "True")
        and any(.status.containerStatuses[]?; .name == "sift" and .imageID == $image)) |
      .metadata.uid'); then
    break
  fi
  sleep 2
done
if [ -z "$new_uid" ]; then
  echo 'Sift replacement pod did not become ready with the same image' >&2
  exit 1
fi

kubectl get pvc "$claim" -o json |
  jq -e --arg uid "$claim_uid" --arg volume "$volume" '
    .metadata.uid == $uid and .status.phase == "Bound" and .spec.volumeName == $volume
  ' >/dev/null
after=$(read_record)
test "$before" = "$after"

jq -cn --arg marker "$marker" --arg old "$original_uid" --arg new "$new_uid" \
  --arg claim "$claim_uid" --arg volume "$volume" --arg image "$image_id" '
  {records: 1, marker: $marker, original_pod_uid: $old, replacement_pod_uid: $new,
   pvc_uid: $claim, volume: $volume, image_id: $image, result: "PASS"}'
