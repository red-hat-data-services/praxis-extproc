#!/usr/bin/env bash
# Configure MetalLB IPAddressPool from the Kind docker network.
# Usage: install-metallb-pool.sh <kube-context>
set -euo pipefail

CTX="${1:?usage: install-metallb-pool.sh <kube-context>}"
ENGINE="${CONTAINER_ENGINE:-$(command -v podman || command -v docker)}"

if kubectl --context "$CTX" get ipaddresspool e2e-pool -n metallb-system &>/dev/null; then
  echo "MetalLB pool already configured"
  exit 0
fi

# Extract the IPv4 subnet from the Kind network. Parse the raw JSON rather than a
# `-f` template: docker exposes it as .IPAM.Config[].Subnet, podman as
# .subnets[].subnet, so a schema-specific template breaks on the other engine.
KIND_SUBNET=$("$ENGINE" network inspect kind 2>/dev/null \
  | grep -ioE '"subnet"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/[0-9]+"' \
  | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/[0-9]+' \
  | head -1)
[[ -n "$KIND_SUBNET" ]] || { echo "cannot determine Kind subnet" >&2; exit 1; }
LB_BASE=$(echo "$KIND_SUBNET" | cut -d'.' -f1-3)

for _ in $(seq 1 6); do
  if kubectl --context "$CTX" apply -f - <<EOF 2>/dev/null
apiVersion: metallb.io/v1beta1
kind: IPAddressPool
metadata:
  name: e2e-pool
  namespace: metallb-system
spec:
  addresses:
  - ${LB_BASE}.200-${LB_BASE}.250
---
apiVersion: metallb.io/v1beta1
kind: L2Advertisement
metadata:
  name: e2e-l2
  namespace: metallb-system
EOF
  then
    echo "MetalLB pool ${LB_BASE}.200-250"
    exit 0
  fi
  echo "MetalLB webhook not ready, retrying..."
  sleep 10
done
echo "failed to configure MetalLB pool" >&2
exit 1
