#!/bin/sh
set -eu
: "${LAYERX_TESTNET_URL:?LAYERX_TESTNET_URL is required}"
: "${LAYERX_GATEWAY_URL:?LAYERX_GATEWAY_URL is required}"
: "${LAYERX_FAUCET_URL:?LAYERX_FAUCET_URL is required}"
: "${LAYERX_TEST_AUTH_TOKEN_FILE:?LAYERX_TEST_AUTH_TOKEN_FILE is required}"
: "${LAYERX_TEST_CA_FILE:?LAYERX_TEST_CA_FILE is required}"
: "${LAYERX_TEST_SOURCE_DID:?LAYERX_TEST_SOURCE_DID is required}"
: "${LAYERX_TEST_SOURCE_PUBLIC_KEY:?LAYERX_TEST_SOURCE_PUBLIC_KEY is required}"
: "${LAYERX_TEST_DESTINATION_DID:?LAYERX_TEST_DESTINATION_DID is required}"
: "${LAYERX_TEST_ASSET:?LAYERX_TEST_ASSET is required}"
: "${LAYERX_TEST_AMOUNT:?LAYERX_TEST_AMOUNT is required}"
: "${LAYERX_BIN:=layerx}"
test -r "$LAYERX_TEST_AUTH_TOKEN_FILE"
test -r "$LAYERX_TEST_CA_FILE"
command -v jq >/dev/null
command -v openssl >/dev/null
command -v "$LAYERX_BIN" >/dev/null
work=$(mktemp -d "${TMPDIR:-/tmp}/layerx-hosted-smoke.XXXXXX")
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
chmod 0700 "$work"
auth_config="$work/auth.curl"
chmod 0600 "$auth_config"
printf 'header = "Authorization: Bearer %s"\n' \
  "$(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")" > "$auth_config"
test "$(wc -c < "$auth_config")" -gt 35
journey="scheduled-$(date -u +%Y%m%dT%H%M%SZ)-$$"

fetch_document() {
  fetch_name=$1
  fetch_url=$2
  curl --silent --show-error --max-time 15 --cacert "$LAYERX_TEST_CA_FILE" \
    --output "$work/$fetch_name.json" --write-out '%{http_code}' "$fetch_url" \
    > "$work/$fetch_name.status"
  jq -e 'type == "object"' "$work/$fetch_name.json" >/dev/null
}

admit_journey() {
  admit_name=$1
  fetch_document "journey-$admit_name" "$LAYERX_TESTNET_URL/v1/journeys/$admit_name"
  if test "$(cat "$work/journey-$admit_name.status")" != 200 \
    || ! jq -e '.admitted == true and .ready == true and (.failing | length) == 0' \
      "$work/journey-$admit_name.json" >/dev/null; then
    printf '%s\n' "journey $admit_name is not admitted:" >&2
    jq -r '"  failing: \(.failing // [] | join(", "))", (.dependencies[]? | select(.ready | not) | "  dependency \(.name): \(.detail)")' \
      "$work/journey-$admit_name.json" >&2
    exit 1
  fi
  printf '%s\n' "journey $admit_name admitted: $(jq -r '[.dependencies[].name] | join(", ")' "$work/journey-$admit_name.json")"
}

fetch_document testnet-readyz "$LAYERX_TESTNET_URL/readyz"
if test "$(cat "$work/testnet-readyz.status")" != 200 \
  || ! jq -e '.state == "ready"' "$work/testnet-readyz.json" >/dev/null; then
  printf '%s\n' "testnet-control is not ready:" >&2
  jq -r '(.release? | select(. != null and .state != "ready") | "  release: \(.detail)"), (.journeys[]? | select(.ready | not) | "  journey \(.journey) degraded: \(.failing | join(", "))"), (.dependencies[]? | select(.ready | not) | "  dependency \(.name): \(.detail)")' \
    "$work/testnet-readyz.json" >&2
  exit 1
fi
jq -e '.service == "layerx-hosted-testnet" and .release.state == "ready"
  and ([.dependencies[].name] | sort) == ["core","core_admin","faucet","gateway","identity","paxeer","receipt_authority","redis","registry"]
  and all(.dependencies[]; .ready == true)
  and ([.journeys[].journey] | sort) == ["funding","payment","programs","receipt_inspection"]
  and all(.journeys[]; .ready == true and (.failing | length) == 0)' \
  "$work/testnet-readyz.json" >/dev/null
jq -r '.dependencies[] | "edge testnet-control -> \(.name): \(.detail)"' "$work/testnet-readyz.json"

fetch_document gateway-readyz "$LAYERX_GATEWAY_URL/readyz"
if test "$(cat "$work/gateway-readyz.status")" != 200 \
  || ! jq -e '.status == "ready" and .service == "layerx-gateway"
      and .components.durable_store == "ready" and .components.core_agent_boundary == "ready"
      and .components.independent_receipt_authority == "ready" and .components.program_registry == "ready"' \
    "$work/gateway-readyz.json" >/dev/null; then
  printf '%s\n' "gateway is not ready:" >&2
  jq -r '.components // {} | to_entries[] | "  component \(.key): \(.value)"' "$work/gateway-readyz.json" >&2
  exit 1
fi
jq -r '.components | to_entries[] | "edge gateway -> \(.key): \(.value)"' "$work/gateway-readyz.json"

fetch_document faucet-readyz "$LAYERX_FAUCET_URL/readyz"
if test "$(cat "$work/faucet-readyz.status")" != 200 \
  || ! jq -e '.status == "ready" and .service == "faucet"' "$work/faucet-readyz.json" >/dev/null; then
  printf '%s\n' "faucet is not ready:" >&2
  cat "$work/faucet-readyz.json" >&2
  exit 1
fi
printf '%s\n' "edge faucet -> redis: ready"

fetch_document parameters "$LAYERX_TESTNET_URL/v1/parameters"
test "$(cat "$work/parameters.status")" = 200
jq -e '.network == "layerx-testnet" and (.network_id | type) == "number"
  and (.package_semver | type) == "string" and (.lxp_wire_protocol_version | type) == "number"' \
  "$work/parameters.json" >/dev/null
jq -e '.network_id == (input | .network_id) and .package_semver == (input | .package_semver)
  and .lxp_wire_protocol_version == (input | .lxp_wire_protocol_version)' \
  "$work/parameters.json" "$work/testnet-readyz.json" >/dev/null
ca_fingerprint=$(openssl x509 -in "$LAYERX_TEST_CA_FILE" -noout -fingerprint -sha256 | tr -d '\r\n')
cluster_identity="cluster identity: $(jq -r '"network=\(.network) network_id=\(.network_id) package_semver=\(.package_semver) lxp_wire_protocol_version=\(.lxp_wire_protocol_version)"' "$work/parameters.json") gateway_network_id=$(jq -r '.network_id' "$work/gateway-readyz.json") gateway_package_semver=$(jq -r '.package_semver' "$work/gateway-readyz.json") testnet=$LAYERX_TESTNET_URL gateway=$LAYERX_GATEWAY_URL faucet=$LAYERX_FAUCET_URL ca=$ca_fingerprint"
printf '%s\n' "$cluster_identity"

admit_journey funding
jq -n --arg did "$LAYERX_TEST_SOURCE_DID" --arg public_key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > "$work/faucet-request.json"
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --config "$auth_config" \
  --request POST "$LAYERX_FAUCET_URL/v1/faucet/claims" \
  --header "Idempotency-Key: faucet-$journey" \
  --header 'Content-Type: application/json' --data-binary "@$work/faucet-request.json" \
  > "$work/faucet-response.json"
jq -e '.funded == true and .funding_id != null' "$work/faucet-response.json" >/dev/null
printf '%s\n' "funding journey: claim faucet-$journey funded"

admit_journey payment
jq -n --arg source "$LAYERX_TEST_SOURCE_DID" --arg destination "$LAYERX_TEST_DESTINATION_DID" \
  --arg currency "$LAYERX_TEST_ASSET" --arg amount "$LAYERX_TEST_AMOUNT" \
  '{source:$source,destination:$destination,money:{currency:$currency,amount:$amount}}' \
  > "$work/quote-request.json"
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --config "$auth_config" \
  --request POST "$LAYERX_GATEWAY_URL/v1/moves/quote" \
  --header 'Content-Type: application/json' \
  --data-binary "@$work/quote-request.json" > "$work/quote-response.json"
quote_id=$(jq -er '.result.quote_id' "$work/quote-response.json")
jq -n --arg quote_id "$quote_id" '{quote_id:$quote_id}' > "$work/payment-request.json"
curl --fail --silent --show-error --max-time 60 --cacert "$LAYERX_TEST_CA_FILE" \
  --config "$auth_config" \
  --request POST "$LAYERX_GATEWAY_URL/v1/moves" \
  --header "Idempotency-Key: payment-$journey" \
  --header 'Content-Type: application/json' --data-binary "@$work/payment-request.json" \
  > "$work/payment-response.json"
receipt_id=$(jq -er '.result.receipt_id' "$work/payment-response.json")
printf '%s\n' "payment journey: quote $quote_id committed as receipt $receipt_id"

admit_journey receipt-inspection
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --config "$auth_config" \
  "$LAYERX_GATEWAY_URL/v1/receipts/$receipt_id" > "$work/receipt-response.json"
jq -er '.result.receipt' "$work/receipt-response.json" > "$work/receipt.hex"
batch_id=$(jq -er '.result.authority.batch_id' "$work/receipt-response.json")
asset=$(jq -er '.result.authority.asset' "$work/receipt-response.json")
previous_root=$(jq -er '.result.authority.previous_state_root' "$work/receipt-response.json")
resulting_root=$(jq -er '.result.authority.resulting_state_root' "$work/receipt-response.json")
sequencer_key=$(jq -er '.result.authority.sequencer_public_key' "$work/receipt-response.json")
"$LAYERX_BIN" --json receipt verify --receipt "$work/receipt.hex" --batch-id "$batch_id" \
  --asset "$asset" --previous-state-root "$previous_root" --resulting-state-root "$resulting_root" \
  --sequencer-public-key "$sequencer_key" > "$work/verification.json"
jq -e '.ok == true and .kind == "receipt.verified" and .data.verified == true' \
  "$work/verification.json" >/dev/null
printf '%s\n' "receipt inspection journey: batch $batch_id receipt $receipt_id independently verified"

admit_journey programs
printf '%s\n' "hosted payment $receipt_id was independently receipt-verified"
printf '%s\n' "$cluster_identity"
