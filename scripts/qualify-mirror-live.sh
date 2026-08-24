#!/usr/bin/env bash
set -euo pipefail

config_path="${LAYERX_MIRROR_LIVE_CONFIG:?set LAYERX_MIRROR_LIVE_CONFIG}"
status_url="${LAYERX_MIRROR_STATUS_URL:-http://127.0.0.1:9091/status}"
publisher_bin="${LAYERX_MIRROR_PUBLISHER_BIN:-interop/target/release/layerx-mirror-publisher}"
fault_controller="${LAYERX_MIRROR_FAULT_CONTROLLER:?set LAYERX_MIRROR_FAULT_CONTROLLER to the authenticated devnet fault-control executable}"

"${publisher_bin}" "${config_path}" &
publisher_pid=$!
cleanup() {
  kill "${publisher_pid}" 2>/dev/null || true
  wait "${publisher_pid}" 2>/dev/null || true
}
trap cleanup EXIT

snapshot() {
  curl --fail --silent --show-error --max-time 5 "${status_url}"
}

wait_for() {
  local expression=$1
  local deadline=$((SECONDS + 1200))
  while (( SECONDS < deadline )); do
    status="$(snapshot || true)"
    if [[ -n "${status}" ]] && jq -e "${expression}" >/dev/null <<<"${status}"; then
      printf '%s' "${status}"
      return 0
    fi
    sleep 5
  done
  snapshot || true
  return 1
}

status="$(wait_for '.ethereum.phase == "retrieved_verified" and .solana.phase == "retrieved_verified" and (.ethereum.latest_batch_mirrored == .solana.latest_batch_mirrored) and (.ethereum.latest_batch_mirrored != null)')"
baseline="$(jq -r '.ethereum.latest_batch_mirrored' <<<"${status}")"

"${fault_controller}" stall ethereum
wait_for ".ethereum.error_class == \"rpc\" and .solana.latest_batch_mirrored > ${baseline}" >/dev/null
"${fault_controller}" restore ethereum
status="$(wait_for '.ethereum.phase == "retrieved_verified" and (.ethereum.latest_batch_mirrored == .solana.latest_batch_mirrored)')"

baseline="$(jq -r '.solana.latest_batch_mirrored' <<<"${status}")"
"${fault_controller}" stall solana
wait_for ".solana.error_class == \"rpc\" and .ethereum.latest_batch_mirrored > ${baseline}" >/dev/null
"${fault_controller}" restore solana
wait_for '.solana.phase == "retrieved_verified" and (.ethereum.latest_batch_mirrored == .solana.latest_batch_mirrored)' >/dev/null

"${fault_controller}" reorg ethereum
wait_for '.ethereum.reorgs_observed > 0' >/dev/null
wait_for '.ethereum.phase == "retrieved_verified" and (.ethereum.latest_batch_mirrored == .solana.latest_batch_mirrored)' >/dev/null

"${fault_controller}" reorg solana
wait_for '.solana.reorgs_observed > 0' >/dev/null
wait_for '.solana.phase == "retrieved_verified" and (.ethereum.latest_batch_mirrored == .solana.latest_batch_mirrored)' >/dev/null
