#!/usr/bin/env bash
set -euo pipefail

BOOTSTRAP_MARKER='<!-- layerx:bootstrap-sequence -->'
CLEAN_BOOTSTRAP_HOME=
CLEAN_BOOTSTRAP_PID_FILE=

cleanup() {
  if [ -n "$CLEAN_BOOTSTRAP_PID_FILE" ] && [ -s "$CLEAN_BOOTSTRAP_PID_FILE" ]; then
    while read -r pid; do
      kill "$pid" 2>/dev/null || true
    done <"$CLEAN_BOOTSTRAP_PID_FILE"
  fi
  if [ -n "$CLEAN_BOOTSTRAP_HOME" ]; then
    rm -rf -- "$CLEAN_BOOTSTRAP_HOME"
  fi
}
trap cleanup EXIT

fail() {
  echo "clean_bootstrap: $*" >&2
  return 1
}

extract_bootstrap_sequence() {
  local document=$1 markers
  markers=$(grep -cxF -- "$BOOTSTRAP_MARKER" "$document" || true)
  [ "$markers" = 1 ] || fail "expected exactly one $BOOTSTRAP_MARKER in $document, found $markers"
  awk -v marker="$BOOTSTRAP_MARKER" '
    $0 == marker { armed = 1; next }
    armed && !inside { if ($0 ~ /^```/) { inside = 1; next } exit 2 }
    inside && $0 == "```" { closed = 1; exit 0 }
    inside { print }
    END { if (!closed) exit 2 }
  ' "$document"
}

flag_value() {
  local sequence=$1 flag=$2
  printf '%s\n' "$sequence" | tr -s ' \t\\' '\n' | awk -v flag="$flag" '$0 == flag { getline; print; exit }'
}

http_get() {
  local authority=$1 path=$2 host port
  host=${authority%:*}
  port=${authority##*:}
  exec 3<>"/dev/tcp/$host/$port"
  printf 'GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n' "$path" "$authority" >&3
  cat <&3
  exec 3<&-
}

clean_bootstrap() {
  local repo_root document binary sequence endpoint network_id authority home
  repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
  document="$repo_root/platform/docs/content/install.md"
  binary=${LAYERX_BIN:-${CARGO_TARGET_DIR:-$repo_root/platform/target}/debug/layerx}
  [ -x "$binary" ] || fail "built layerx binary not found at $binary (set LAYERX_BIN or CARGO_TARGET_DIR)"
  sequence=$(extract_bootstrap_sequence "$document")
  [ -n "$sequence" ] || fail "the bootstrap block in $document is empty"
  endpoint=$(flag_value "$sequence" --endpoint)
  network_id=$(flag_value "$sequence" --network-id)
  { [ -n "$endpoint" ] && [ -n "$network_id" ]; } || fail "the bootstrap block does not pass --endpoint and --network-id"
  authority=${endpoint#http://}
  authority=${authority%%/*}

  CLEAN_BOOTSTRAP_HOME=$(mktemp -d "${TMPDIR:-/tmp}/layerx-clean-bootstrap.XXXXXX")
  home=$CLEAN_BOOTSTRAP_HOME
  CLEAN_BOOTSTRAP_PID_FILE="$home/emulator.pid"
  local pid_file=$CLEAN_BOOTSTRAP_PID_FILE transcript="$home/transcript.log" status=0 started elapsed
  mkdir "$home/bin"
  ln -s "$binary" "$home/bin/layerx"
  {
    printf 'set -euo pipefail\n'
    printf "trap 'jobs -p >\"%s\"' EXIT\n" "$pid_file"
    printf '%s\n' "$sequence"
  } >"$home/sequence.sh"

  started=$(date +%s)
  env -i HOME="$home" PATH="$home/bin:$PATH" bash "$home/sequence.sh" >"$transcript" 2>&1 || status=$?
  elapsed=$(( $(date +%s) - started ))
  if [ "$status" -ne 0 ]; then
    cat "$transcript" >&2
    fail "the published bootstrap sequence exited with status $status"
  fi

  local layerx_home="$home/.config/layerx" seed_file anchor_file seed_hex anchor perm
  seed_file="$layerx_home/emulator/sequencer.seed"
  anchor_file="$layerx_home/emulator/sequencer.anchor"
  [ -f "$seed_file" ] || fail "provisioning did not write $seed_file"
  [ -f "$anchor_file" ] || fail "provisioning did not write $anchor_file"
  perm=$(ls -ld -- "$seed_file" | cut -c1-10)
  [ "$perm" = "-rw-------" ] || fail "sequencer seed should be owner-only, found $perm"
  perm=$(ls -ld -- "$layerx_home/emulator" | cut -c1-10)
  [ "$perm" = "drwx------" ] || fail "emulator profile directory should be owner-only, found $perm"
  seed_hex=$(tr -d '\r\n' <"$seed_file")
  [ "${#seed_hex}" -eq 64 ] || fail "sequencer seed file should hold 64 hex characters"
  if grep -qiF -- "$seed_hex" "$transcript"; then
    fail "seed material appeared in the bootstrap transcript"
  fi
  anchor=$(tr -d '\r\n' <"$anchor_file")
  [ "${#anchor}" -eq 64 ] || fail "sequencer trust anchor file should hold 64 hex characters"

  local health identity current
  health=$(http_get "$authority" /healthz)
  printf '%s' "$health" | grep -qF '"status":"ready"' || fail "emulator at $endpoint is not ready: $health"
  identity=$(http_get "$authority" /v1/sequencer)
  printf '%s' "$identity" | grep -qF "\"network_id\":$network_id" || fail "emulator does not advertise network id $network_id: $identity"
  printf '%s' "$identity" | grep -qF "\"sequencer_public_key\":\"$anchor\"" || fail "emulator identity disagrees with the published anchor: $identity"
  current=$(env -i HOME="$home" PATH="$home/bin:$PATH" layerx --json environment current)
  printf '%s' "$current" | grep -qF '"name":"emulator"' || fail "emulator is not the current environment: $current"
  printf '%s' "$current" | grep -qF "\"endpoint\":\"$endpoint\"" || fail "current environment endpoint is not $endpoint: $current"
  printf '%s' "$current" | grep -qF "\"network_id\":$network_id" || fail "current environment network id is not $network_id: $current"
  printf '%s' "$current" | grep -qF "\"sequencer_trust_anchor\":\"$anchor\"" || fail "current environment does not bind the published anchor: $current"
  printf 'clean_bootstrap: emulator reachable at %s, environment emulator current, anchor %s bound, elapsed_seconds=%s\n' "$endpoint" "$anchor" "$elapsed"
}

clean_bootstrap "$@"
