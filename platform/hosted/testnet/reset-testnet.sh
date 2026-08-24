#!/bin/sh
set -eu
command -v curl >/dev/null
command -v jq >/dev/null
: "${LAYERX_TESTNET_ADMIN_URL:?LAYERX_TESTNET_ADMIN_URL is required}"
: "${LAYERX_TESTNET_RESET_TOKEN_FILE:?LAYERX_TESTNET_RESET_TOKEN_FILE is required}"
: "${LAYERX_TESTNET_CA_FILE:?LAYERX_TESTNET_CA_FILE is required}"
test -r "$LAYERX_TESTNET_RESET_TOKEN_FILE"
test -r "$LAYERX_TESTNET_CA_FILE"
day_of_month=$(date -u +%d)
day_of_month=${day_of_month#0}
test "$day_of_month" -le 7 || exit 0
request_id="scheduled-reset-$(date -u +%Y%m)"
curl_config=$(mktemp /tmp/layerx-reset.XXXXXX)
response_file=$(mktemp /tmp/layerx-reset-response.XXXXXX)
trap 'rm -f -- "$curl_config" "$response_file"' EXIT HUP INT TERM
chmod 0600 "$curl_config" "$response_file"
{
  printf 'fail\n'
  printf 'silent\n'
  printf 'show-error\n'
  printf 'max-time = 30\n'
  printf 'cacert = "%s"\n' "$LAYERX_TESTNET_CA_FILE"
  printf 'request = "POST"\n'
  printf 'header = "Authorization: Bearer %s"\n' "$(tr -d '\r\n' < "$LAYERX_TESTNET_RESET_TOKEN_FILE")"
  printf 'header = "Idempotency-Key: %s"\n' "$request_id"
  printf 'header = "Content-Type: application/json"\n'
  printf 'data = "{}"\n'
  printf 'output = "%s"\n' "$response_file"
  printf 'url = "%s/admin/v1/testnet/reset"\n' "$LAYERX_TESTNET_ADMIN_URL"
} > "$curl_config"
curl --config "$curl_config"
jq -e '.state == "reset" and (.reset_id | type == "string" and length > 0)' "$response_file" >/dev/null
