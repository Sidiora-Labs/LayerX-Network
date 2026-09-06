#!/bin/sh
set -eu

: "${LAYERX_GATEWAY_URL:?real hosted gateway URL is required}"
: "${LAYERX_GATEWAY_CA_FILE:?gateway CA is required}"
: "${LAYERX_GATEWAY_KEY_ID:?gateway key ID is required}"
: "${LAYERX_GATEWAY_KEY_SECRET:?gateway key secret is required}"
: "${LAYERX_RECEIPT_VERIFY_BIN:?offline receipt verifier is required}"
: "${LAYERX_LIFECYCLE_MANIFEST:?manifest of real signed lifecycle activities is required}"

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
authorization="LayerX-Key ${LAYERX_GATEWAY_KEY_ID}:${LAYERX_GATEWAY_KEY_SECRET}"
jq -e 'map(.route) == ["deploy", "upgrade", "wind-down"] and all(.[];
  (.activity_id | test("^[0-9a-f]{64}$")) and
  (.idempotency_key | test("^[0-9a-f]{64}$")) and
  (.signed_file | type == "string"))' "$LAYERX_LIFECYCLE_MANIFEST" >/dev/null
jq -c '.[]' "$LAYERX_LIFECYCLE_MANIFEST" | while IFS= read -r operation; do
  route=$(printf '%s' "$operation" | jq -er '.route')
  file=$(printf '%s' "$operation" | jq -er '.signed_file')
  key=$(printf '%s' "$operation" | jq -er '.idempotency_key')
  activity_id=$(printf '%s' "$operation" | jq -er '.activity_id')
  url="$LAYERX_GATEWAY_URL/v1/programs/$route"
  test -s "$file"
  for mutation_path in activities programs/call programs/deploy programs/upgrade programs/wind-down; do
    for media_type in application/json text/plain 'application/octet-stream; charset=utf-8'; do
      status=$(curl --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" -o "$directory/media.json" -w '%{http_code}' \
        -H "Authorization: $authorization" -H "Content-Type: $media_type" -H "Idempotency-Key: $key" --data-binary "@$file" "$LAYERX_GATEWAY_URL/v1/$mutation_path")
      if test "$status" != 415; then
        printf 'media-type refusal failed for %s at %s: HTTP %s\n' "$media_type" "$mutation_path" "$status" >&2
        exit 1
      fi
    done
  done
  test "$(curl --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" -o "$directory/unauthorized.json" -w '%{http_code}' \
    -H 'Content-Type: application/octet-stream' -H "Idempotency-Key: $key" --data-binary "@$file" "$url")" = 401
  test "$(curl --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" -o "$directory/content.json" -w '%{http_code}' \
    -H "Authorization: $authorization" -H 'Content-Type: application/json' -H "Idempotency-Key: $key" --data-binary "@$file" "$url")" = 415
  test "$(curl --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" -o "$directory/key.json" -w '%{http_code}' \
    -H "Authorization: $authorization" -H 'Content-Type: application/octet-stream' --data-binary "@$file" "$url")" = 400
  python3 - "$file" "$directory/bad-signature.bin" <<'PY'
import pathlib
import sys
activity = bytearray(pathlib.Path(sys.argv[1]).read_bytes())
activity[-1] ^= 1
pathlib.Path(sys.argv[2]).write_bytes(activity)
PY
  test "$(curl --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" -o "$directory/signature.json" -w '%{http_code}' \
    -H "Authorization: $authorization" -H 'Content-Type: application/octet-stream' -H "Idempotency-Key: $key" \
    --data-binary "@$directory/bad-signature.bin" "$url")" = 403
  for attempt in first replay; do
    curl --fail-with-body --silent --show-error --cacert "$LAYERX_GATEWAY_CA_FILE" \
      -H "Authorization: $authorization" -H 'Content-Type: application/octet-stream' \
      -H "Idempotency-Key: $key" --data-binary "@$file" "$url" > "$directory/$attempt.json"
    jq -e --arg id "$activity_id" '.result.activity_id == $id and
      (.result.receipt | test("^[0-9a-f]+$")) and (.result | has("program_id") | not)' "$directory/$attempt.json" >/dev/null
    jq -er '.result.receipt' "$directory/$attempt.json" | xxd -r -p > "$directory/receipt.bin"
    "$LAYERX_RECEIPT_VERIFY_BIN" "$directory/receipt.bin"
    jq -S '.result' "$directory/$attempt.json" > "$directory/$attempt.result"
  done
  cmp "$directory/first.result" "$directory/replay.result"
done
