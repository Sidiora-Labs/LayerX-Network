#!/usr/bin/env bash
set -euo pipefail

: "${WEBHOOKS_URL:?HTTPS webhook service URL is required}"
: "${WEBHOOK_ENDPOINT_ID:?registered real endpoint is required}"
: "${WEBHOOK_SESSION_COOKIE_FILE:?protected developer session cookie file is required}"
: "${WEBHOOK_CSRF_FILE:?anti-forgery token file is required}"
: "${WEBHOOK_SOURCE_TRIGGER_TOKEN_FILE:?source trigger token file is required}"
: "${WEBHOOK_SOURCE_EVENT_FIRST:?first canonical source event id is required}"
: "${WEBHOOK_SOURCE_EVENT_SECOND:?second canonical source event id is required}"
: "${WEBHOOK_SOURCE_EVENT_STALE:?canonical event older than the second event is required}"
: "${WEBHOOK_SOURCE_KIND:?journey, payment, approval or program is required}"
: "${WEBHOOK_RECEIVER_OBSERVATIONS_URL:?real receiver observation API is required}"
: "${WEBHOOK_RECEIVER_TOKEN_FILE:?real receiver observation token is required}"

session_cookie="$(<"${WEBHOOK_SESSION_COOKIE_FILE}")"
csrf="$(<"${WEBHOOK_CSRF_FILE}")"
source_token="$(<"${WEBHOOK_SOURCE_TRIGGER_TOKEN_FILE}")"
receiver_token="$(<"${WEBHOOK_RECEIVER_TOKEN_FILE}")"

cursor="$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
  -H "Cookie: __Host-layerx-session=${session_cookie}" \
  "${WEBHOOKS_URL}/v1/webhooks/endpoints/${WEBHOOK_ENDPOINT_ID}/events?limit=1" | jq -er .next_cursor)"

curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
  -X POST -H "Authorization: Bearer ${source_token}" \
  "${WEBHOOKS_URL}/internal/v1/events/${WEBHOOK_SOURCE_KIND}/${WEBHOOK_SOURCE_EVENT_FIRST}" >/dev/null

kubectl delete pod -l app=layerx-webhooks --field-selector=status.phase=Running --wait=false

curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
  -X POST -H "Authorization: Bearer ${source_token}" \
  "${WEBHOOKS_URL}/internal/v1/events/${WEBHOOK_SOURCE_KIND}/${WEBHOOK_SOURCE_EVENT_SECOND}" >/dev/null

duplicate="$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
  -X POST -H "Authorization: Bearer ${source_token}" \
  "${WEBHOOKS_URL}/internal/v1/events/${WEBHOOK_SOURCE_KIND}/${WEBHOOK_SOURCE_EVENT_FIRST}")"
jq -e '.duplicate == true' <<<"${duplicate}" >/dev/null

stale_status="$(curl --silent --show-error --proto '=https' --tlsv1.2 \
  -o /dev/null -w '%{http_code}' \
  -X POST -H "Authorization: Bearer ${source_token}" \
  "${WEBHOOKS_URL}/internal/v1/events/${WEBHOOK_SOURCE_KIND}/${WEBHOOK_SOURCE_EVENT_STALE}")"
[[ "${stale_status}" == "409" ]]

deadline="$((SECONDS + 180))"
while (( SECONDS < deadline )); do
  observations="$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
    -H "Authorization: Bearer ${receiver_token}" \
    "${WEBHOOK_RECEIVER_OBSERVATIONS_URL}?endpoint=${WEBHOOK_ENDPOINT_ID}")"
  if jq -e --arg first "${WEBHOOK_SOURCE_EVENT_FIRST}" --arg second "${WEBHOOK_SOURCE_EVENT_SECOND}" \
    '([.deliveries[] | select(.source_event_id == $first)] | length) > 0 and ([.deliveries[] | select(.source_event_id == $second)] | length) > 0' \
    <<<"${observations}" >/dev/null; then
    break
  fi
  sleep 2
done

jq -e --arg first "${WEBHOOK_SOURCE_EVENT_FIRST}" --arg second "${WEBHOOK_SOURCE_EVENT_SECOND}" '
  [.deliveries[] | select(.source_event_id == $first or .source_event_id == $second)] as $selected |
  ($selected | length) >= 2 and
  ([range(0; ($selected | length)) | select($selected[.].source_event_id == $first)] | min) <
    ([range(0; ($selected | length)) | select($selected[.].source_event_id == $second)] | min) and
  ([$selected[] | {event_id, body_digest}] | group_by(.event_id) | all(map(.body_digest) | unique | length == 1))
' <<<"${observations}" >/dev/null

curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
  -X POST \
  -H "Cookie: __Host-layerx-session=${session_cookie}" \
  -H "X-LayerX-CSRF: ${csrf}" \
  -H "Idempotency-Key: fault-redelivery-${WEBHOOK_SOURCE_EVENT_FIRST}" \
  "${WEBHOOKS_URL}/v1/webhooks/endpoints/${WEBHOOK_ENDPOINT_ID}/redeliveries?cursor=${cursor}&limit=200" >/dev/null

deadline="$((SECONDS + 180))"
while (( SECONDS < deadline )); do
  observations="$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 \
    -H "Authorization: Bearer ${receiver_token}" \
    "${WEBHOOK_RECEIVER_OBSERVATIONS_URL}?endpoint=${WEBHOOK_ENDPOINT_ID}")"
  if jq -e --arg first "${WEBHOOK_SOURCE_EVENT_FIRST}" \
    '([.deliveries[] | select(.source_event_id == $first)] | length) >= 2' \
    <<<"${observations}" >/dev/null; then
    break
  fi
  sleep 2
done

jq -e --arg first "${WEBHOOK_SOURCE_EVENT_FIRST}" '
  [.deliveries[] | select(.source_event_id == $first)] as $duplicates |
  ($duplicates | length) >= 2 and
  ([$duplicates[].body_digest] | unique | length) == 1
' <<<"${observations}" >/dev/null
