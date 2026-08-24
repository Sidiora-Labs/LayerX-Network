#!/usr/bin/env bash
set -euo pipefail

verifier_bin="${LAYERX_MIRROR_VERIFY_BIN:?set LAYERX_MIRROR_VERIFY_BIN}"
config_path="${LAYERX_MIRROR_VERIFY_CONFIG:?set LAYERX_MIRROR_VERIFY_CONFIG}"
canonical_request="${LAYERX_MIRROR_CANONICAL_REQUEST:?set LAYERX_MIRROR_CANONICAL_REQUEST}"
failover_request="${LAYERX_MIRROR_FAILOVER_REQUEST:?set LAYERX_MIRROR_FAILOVER_REQUEST}"
divergence_request="${LAYERX_MIRROR_DIVERGENCE_REQUEST:?set LAYERX_MIRROR_DIVERGENCE_REQUEST}"
tamper_request="${LAYERX_MIRROR_TAMPER_REQUEST:?set LAYERX_MIRROR_TAMPER_REQUEST}"

[[ -x "${verifier_bin}" && -r "${config_path}" && -r "${canonical_request}" && -r "${failover_request}" && -r "${divergence_request}" && -r "${tamper_request}" ]]
[[ -z "${LAYERX_NODE_URL:-}" && -z "${LAYERX_GATEWAY_URL:-}" && -z "${LAYERX_EXPLORER_API_ORIGIN:-}" ]]

verify_ok() {
  local request_path=$1
  "${verifier_bin}" "${config_path}" < "${request_path}" | jq -ce 'select(.ok == true and .verification.provenance == "Canonical" and .verification.sourceId != "")'
}

verify_error() {
  local request_path=$1
  local expected=$2
  "${verifier_bin}" "${config_path}" < "${request_path}" | jq -e --arg expected "${expected}" '.ok == false and .error == $expected'
}

verify_ok "${canonical_request}"
verify_ok "${failover_request}" | jq -e '.verification.failoverCount > 0'
verify_error "${divergence_request}" divergent
verify_error "${tamper_request}" verification

for command_name in LAYERX_MIRROR_TS_CONFORMANCE LAYERX_MIRROR_PYTHON_CONFORMANCE LAYERX_MIRROR_GO_CONFORMANCE LAYERX_MIRROR_JVM_CONFORMANCE LAYERX_MIRROR_SWIFT_CONFORMANCE LAYERX_MIRROR_DOTNET_CONFORMANCE; do
  command_value="${!command_name:?set ${command_name}}"
  env -u LAYERX_NODE_URL -u LAYERX_GATEWAY_URL -u LAYERX_EXPLORER_API_ORIGIN bash -euo pipefail -c "${command_value}"
done
