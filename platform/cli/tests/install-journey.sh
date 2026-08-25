#!/usr/bin/env bash
set -euo pipefail

: "${LAYERX_BIN:?set LAYERX_BIN to the built layerx executable}"
: "${LAYERX_GATEWAY_URL:?set the hosted gateway URL}"
: "${LAYERX_NETWORK_ID:?set the hosted protocol network id}"
: "${LAYERX_IDENTITY_TOKEN:?set a short-lived identity session}"
: "${LAYERX_SIGNING_SEED:?set the funded source Ed25519 seed}"
: "${LAYERX_SOURCE_ACCOUNT:?set the funded 64-hex source account}"
: "${LAYERX_PAYMENT_ASSET:?set the 64-hex payment asset}"
: "${LAYERX_PAYMENT_DESTINATION:?set the 64-hex destination account}"
: "${LAYERX_MCP_SEQUENCE:?set the current source sequence for the MCP payment}"
: "${LAYERX_A2A_SEQUENCE:?set the next source sequence for the A2A payment}"

journey_root=$(mktemp -d)
cleanup() {
  LAYERX_CONFIG="$journey_root/config.json" LAYERX_INSTALL_ROOT="$journey_root" \
    "$LAYERX_BIN" a2a stop >/dev/null 2>&1 || true
  rm -rf "$journey_root"
}
trap cleanup EXIT INT TERM
umask 077

export LAYERX_CONFIG="$journey_root/config.json"
export LAYERX_INSTALL_ROOT="$journey_root"

"$LAYERX_BIN" --json environment use testnet \
  --endpoint "$LAYERX_GATEWAY_URL" --network-id "$LAYERX_NETWORK_ID" >/dev/null
printf '%s\n' "$LAYERX_SIGNING_SEED" | \
  "$LAYERX_BIN" --json key import agent-runtime >/dev/null

printf '%s\n' "$LAYERX_IDENTITY_TOKEN" | \
  "$LAYERX_BIN" --json install mcp --environment testnet --host layerx \
    --key agent-runtime --source-account "$LAYERX_SOURCE_ACCOUNT" \
    --asset "$LAYERX_PAYMENT_ASSET" --token-stdin >"$journey_root/mcp-install.json"

now_ms=$(($(date +%s) * 1000))
expires_ms=$((now_ms + 120000))
mcp_idempotency=$(openssl rand -hex 32)
mcp_call=$(jq -nc \
  --arg destination "$LAYERX_PAYMENT_DESTINATION" \
  --arg sequence "$LAYERX_MCP_SEQUENCE" \
  --arg not_before "$now_ms" \
  --arg expires "$expires_ms" \
  --arg idempotency "$mcp_idempotency" \
  '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:"activity.submit",arguments:{destination:$destination,amount:"1",account_sequence:$sequence,not_before_ms:$not_before,expires_at_ms:$expires,fee_limit:"1000",idempotency_key:$idempotency}}}')
mcp_command=$(jq -er '.mcpServers.layerx.command' "$journey_root/mcp.json")
mapfile -t mcp_arguments < <(jq -er '.mcpServers.layerx.args[]' "$journey_root/mcp.json")
test "$(jq -er '.mcpServers.layerx.env.LAYERX_CONFIG' "$journey_root/mcp.json")" = "$LAYERX_CONFIG"
export LAYERX_GATEWAY_KEY_ID
LAYERX_GATEWAY_KEY_ID=$(jq -er '.mcpServers.layerx.env.LAYERX_GATEWAY_KEY_ID' "$journey_root/mcp.json")
{
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
  printf '%s\n' "$mcp_call"
} | "$mcp_command" "${mcp_arguments[@]}" >"$journey_root/mcp-runtime.jsonl"
mcp_activity=$(tail -n 1 "$journey_root/mcp-runtime.jsonl" | \
  jq -er '.result.structuredContent.result.gateway.result.activity_id')
test "$(printf '%s' "$mcp_activity" | wc -c)" -eq 64

a2a_port="${LAYERX_A2A_PORT:-19433}"
"$LAYERX_BIN" --json install a2a --environment testnet --key agent-runtime \
  --source-account "$LAYERX_SOURCE_ACCOUNT" --asset "$LAYERX_PAYMENT_ASSET" \
  --listen "127.0.0.1:$a2a_port" >"$journey_root/a2a-install.json"
jq -e '.data.lifecycle.state == "running"' "$journey_root/a2a-install.json" >/dev/null
a2a_authorization_file=$(jq -er '.data.authorization.credential_file' "$journey_root/a2a-install.json")
a2a_authorization=$(tr -d '\r\n' <"$a2a_authorization_file")
"$LAYERX_BIN" --json a2a stop | jq -e '.data.state == "stopped"' >/dev/null
"$LAYERX_BIN" --json a2a start | jq -e '.data.state == "running"' >/dev/null

ready=false
for _ in $(seq 1 50); do
  if curl --silent --fail "http://127.0.0.1:$a2a_port/.well-known/agent-card.json" \
    | jq -e '.name == "LayerX Payment Agent"' >/dev/null; then
    ready=true
    break
  fi
  sleep 0.1
done
test "$ready" = true

now_ms=$(($(date +%s) * 1000))
expires_ms=$((now_ms + 120000))
a2a_idempotency=$(openssl rand -hex 32)
a2a_request=$(jq -nc \
  --arg destination "$LAYERX_PAYMENT_DESTINATION" \
  --arg sequence "$LAYERX_A2A_SEQUENCE" \
  --arg not_before "$now_ms" \
  --arg expires "$expires_ms" \
  --arg idempotency "$a2a_idempotency" \
  '{jsonrpc:"2.0",id:1,method:"message/send",params:{message:{kind:"message",role:"user",messageId:"install-journey",parts:[{kind:"data",data:{skill:"activity.submit",arguments:{destination:$destination,amount:"1",account_sequence:$sequence,not_before_ms:$not_before,expires_at_ms:$expires,fee_limit:"1000",idempotency_key:$idempotency}}}]}}}')
a2a_response=$(curl --fail-with-body --silent --show-error \
  -H 'Content-Type: application/json' -H "Authorization: Bearer $a2a_authorization" \
  --data "$a2a_request" "http://127.0.0.1:$a2a_port/")
a2a_activity=$(printf '%s' "$a2a_response" | \
  jq -er '.result.artifacts[0].parts[0].data.result.gateway.result.activity_id')
test "$(printf '%s' "$a2a_activity" | wc -c)" -eq 64

"$LAYERX_BIN" --json a2a status | jq -e '.data.state == "running"' >/dev/null
"$LAYERX_BIN" --json a2a stop | jq -e '.data.state == "stopped"' >/dev/null
