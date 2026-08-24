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
command -v "$LAYERX_BIN" >/dev/null
work=$(mktemp -d /tmp/layerx-hosted-smoke.XXXXXX)
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
chmod 0700 "$work"
auth_config="$work/auth.curl"
chmod 0600 "$auth_config"
printf 'header = "Authorization: Bearer %s"\n' \
  "$(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")" > "$auth_config"
test "$(wc -c < "$auth_config")" -gt 35
journey="scheduled-$(date -u +%Y%m%dT%H%M%SZ)-$$"

curl --fail --silent --show-error --max-time 15 --cacert "$LAYERX_TEST_CA_FILE" \
  "$LAYERX_TESTNET_URL/readyz" > "$work/readiness.json"
jq -e '.state == "ready" and ([.components[].name] | sort) == (["core","gateway","paxeer","testnet"] | sort)' \
  "$work/readiness.json" >/dev/null

jq -n --arg did "$LAYERX_TEST_SOURCE_DID" --arg public_key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > "$work/faucet-request.json"
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --config "$auth_config" \
  --request POST "$LAYERX_FAUCET_URL/v1/faucet/claims" \
  --header "Idempotency-Key: faucet-$journey" \
  --header 'Content-Type: application/json' --data-binary "@$work/faucet-request.json" \
  > "$work/faucet-response.json"
jq -e '.funded == true and .funding_id != null' "$work/faucet-response.json" >/dev/null

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
printf '%s\n' "hosted payment $receipt_id was independently receipt-verified"
