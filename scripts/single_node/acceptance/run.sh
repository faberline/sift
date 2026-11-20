#!/usr/bin/env bash
# Setup phase of the single-node acceptance run. The aw reusable workflow
# calls this script with NAMESPACE, IMAGE, EVIDENCE_DIR, RUN_LABEL and
# KUBECONFIG set. It writes setup.json; the aw workflow tears everything down.
set -euo pipefail

: "${NAMESPACE:?}" "${EVIDENCE_DIR:?}" "${IMAGE:?}"
start="$(date -u +%FT%TZ)"
mkdir -p "$EVIDENCE_DIR"

# 1. The operator makes the ServiceAccount sift. Give it the pull secret, then
#    delete a Pod that may have been made before the patch.
deadline=$((SECONDS + 300))
until kubectl -n "$NAMESPACE" get serviceaccount sift >/dev/null 2>&1; do
  if ((SECONDS >= deadline)); then
    echo "the operator did not make the ServiceAccount sift" >&2
    exit 1
  fi
  sleep 5
done
kubectl -n "$NAMESPACE" patch serviceaccount sift --type merge \
  -p '{"imagePullSecrets":[{"name":"ghcr-pull"}]}'
kubectl -n "$NAMESPACE" delete pod sift-all-0 --ignore-not-found

# 2. Wait for the Pod to be Ready. Fail fast when the store PVC does not bind.
deadline=$((SECONDS + 900))
while true; do
  ready="$(kubectl -n "$NAMESPACE" get pod sift-all-0 \
    -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}' 2>/dev/null || true)"
  if [[ "$ready" == "True" ]]; then
    break
  fi
  if [[ -n "$(kubectl -n "$NAMESPACE" get events \
    --field-selector involvedObject.name=data-sift-all-0,reason=ProvisioningFailed \
    -o name 2>/dev/null)" ]]; then
    kubectl -n "$NAMESPACE" describe pvc data-sift-all-0 || true
    echo "the store PVC data-sift-all-0 (242Gi, class dynamic-rwo) did not bind: quota or storage class. Do not ask for a quota; report blocked with this figure." >&2
    exit 1
  fi
  if ((SECONDS >= deadline)); then
    kubectl -n "$NAMESPACE" describe pod sift-all-0 || true
    echo "Pod sift-all-0 was not Ready in 900 s" >&2
    exit 1
  fi
  sleep 10
done

# 3. Write the setup result.
end="$(date -u +%FT%TZ)"
nodes="$(kubectl get nodes -l faberline.dev/pool=sift-server --no-headers | wc -l)"
jq -n --arg start "$start" --arg end "$end" --argjson nodes "$nodes" --arg image "$IMAGE" \
  '{phase: "setup", node_count: $nodes, start: $start, end: $end, image: $image, pod: "sift-all-0", pvc: "data-sift-all-0"}' \
  > "$EVIDENCE_DIR/setup.json"
