#!/bin/sh
set -eu
command -v curl >/dev/null
command -v jq >/dev/null
: "${LAYERX_TESTNET_STATUS_URL:?LAYERX_TESTNET_STATUS_URL is required}"
: "${LAYERX_STATUS_PUBLISH_URL:?LAYERX_STATUS_PUBLISH_URL is required}"
: "${LAYERX_STATUS_TOKEN_FILE:?LAYERX_STATUS_TOKEN_FILE is required}"
: "${LAYERX_TESTNET_CA_FILE:?LAYERX_TESTNET_CA_FILE is required}"
test -r "$LAYERX_STATUS_TOKEN_FILE"
test -r "$LAYERX_TESTNET_CA_FILE"
status_file=$(mktemp /tmp/layerx-status.XXXXXX)
curl_config=$(mktemp /tmp/layerx-status-curl.XXXXXX)
trap 'rm -f -- "$status_file" "$curl_config"' EXIT HUP INT TERM
chmod 0600 "$status_file" "$curl_config"
curl --fail --silent --show-error --max-time 15 --cacert "$LAYERX_TESTNET_CA_FILE" \
  --output "$status_file" "$LAYERX_TESTNET_STATUS_URL"
test "$(wc -c < "$status_file")" -le 65536
jq -e '
  type == "object" and
  ((keys | sort) == (["components","lxp_wire_protocol_version","network_id","package_semver","service","state"] | sort)) and
  (.service == "layerx-hosted-testnet") and
  (.state == "ready" or .state == "degraded") and
  ([.components[].name] | sort) == (["core","gateway","paxeer","testnet"] | sort) and
  all(.components[]; ((keys | sort) == ["name","state"]) and (.state == "ready" or .state == "degraded" or .state == "unavailable"))
' "$status_file" >/dev/null
{
  printf 'fail\n'
  printf 'silent\n'
  printf 'show-error\n'
  printf 'max-time = 15\n'
  printf 'cacert = "%s"\n' "$LAYERX_TESTNET_CA_FILE"
  printf 'request = "PUT"\n'
  printf 'header = "Authorization: Bearer %s"\n' "$(tr -d '\r\n' < "$LAYERX_STATUS_TOKEN_FILE")"
  printf 'header = "Content-Type: application/json"\n'
  printf 'data-binary = "@%s"\n' "$status_file"
  printf 'url = "%s"\n' "$LAYERX_STATUS_PUBLISH_URL"
} > "$curl_config"
curl --config "$curl_config"
